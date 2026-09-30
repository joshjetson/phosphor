//! A chop being set up: the sound, how it is cut, and where it will land.
//!
//! Nothing in the kit changes until [`super::land::land`] is called with
//! what this holds. The plan carries its own handle on the source sound, so
//! an undo or a pad edit behind it cannot pull the audio out from under the
//! screen — the landing re-checks the kit it lands on, which is the one
//! thing that may have moved.
//!
//! Detection is cached per band. Reading a band walks every frame of the
//! recording, and turning the sensitivity must not — so each band is read
//! the first time it is listened through and every later pick is a walk of
//! 5 ms hops. See [`super::onset`].

use std::sync::Arc;

use phosphor_plugin::sample::{SamplePcm, NUM_PADS};

use super::grid::{self, Division};
use super::land::{self, Feel};
use super::markers::{Marker, Markers};
use super::onset::{OnsetCurve, DEFAULT_SENSITIVITY};
use super::Band;
use crate::sampler::{LayerState, SamplerState};

/// How the cuts are decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChopMode {
    /// Where the hits are, heard through a band.
    #[default]
    Transient,
    /// A loop of so many bars, cut at a beat division.
    Grid,
    /// So many slices of the same length.
    Equal,
}

impl ChopMode {
    pub const ALL: [ChopMode; 3] = [Self::Transient, Self::Grid, Self::Equal];

    pub fn label(self) -> &'static str {
        match self {
            Self::Transient => "transient",
            Self::Grid => "grid",
            Self::Equal => "equal",
        }
    }
}

/// The key slice one lands on by default: MIDI 36 — C2 here, where middle C
/// is C4 — the key Ableton's and Logic's slicers put it on, which they call
/// C1 because their middle C is C3. The same key under the hand either way.
pub const DEFAULT_FIRST_NOTE: u8 = 36;

/// Equal slices a fresh plan offers.
pub const DEFAULT_SLICES: u32 = 8;

/// Longest loop the grid is told about.
pub const MAX_BARS: u32 = 64;

#[derive(Debug, Clone)]
pub struct ChopPlan {
    /// The sound being chopped: every slice is this layer with its own
    /// window. Its audio is present — [`ChopPlan::new`] refuses otherwise.
    source: LayerState,
    pcm: Arc<SamplePcm>,
    /// Where in the buffer the chop is taken from: the source's own trim,
    /// so a player who trimmed off the count-in chops what they kept.
    region: (u64, u64),
    pub mode: ChopMode,
    pub band: Band,
    /// 0..=1.
    pub sensitivity: f32,
    /// Transient only: keep at most this many cuts, the strongest.
    pub fit: Option<usize>,
    pub bars: u32,
    pub division: Division,
    /// Equal only.
    pub slices: u32,
    /// The pad slice one lands on.
    pub first: usize,
    pub feel: Feel,
    /// Also write a clip that replays the recording through the slices.
    /// See [`super::replay`].
    pub clip: bool,
    markers: Markers,
    /// How many cuts detection proposed before a fit thinned them — what a
    /// fit is walked against.
    proposed: usize,
    /// The slice under the cursor, by its cut.
    pub selected: usize,
    curves: [Option<OnsetCurve>; 4],
}

fn band_slot(band: Band) -> usize {
    match band {
        Band::Full => 0,
        Band::Low => 1,
        Band::Mid => 2,
        Band::High => 3,
    }
}

impl ChopPlan {
    /// A plan over `source`, cut at its hits at the default sensitivity.
    /// `None` when the sound has no audio behind it to cut.
    pub fn new(source: LayerState) -> Option<Self> {
        let pcm = Arc::clone(source.pcm.as_ref()?);
        let region = source.region()?;
        let mut plan = Self {
            markers: Markers::new(region.0, region.1, pcm.sample_rate),
            source,
            pcm,
            region,
            mode: ChopMode::default(),
            band: Band::default(),
            sensitivity: DEFAULT_SENSITIVITY,
            fit: None,
            bars: 1,
            division: Division::default(),
            slices: DEFAULT_SLICES,
            first: SamplerState::pad_of_note(DEFAULT_FIRST_NOTE).unwrap_or(0),
            feel: Feel::default(),
            clip: false,
            proposed: 0,
            selected: 0,
            curves: Default::default(),
        };
        plan.redetect();
        Some(plan)
    }

    pub fn source(&self) -> &LayerState {
        &self.source
    }

    pub fn pcm(&self) -> &Arc<SamplePcm> {
        &self.pcm
    }

    pub fn region(&self) -> (u64, u64) {
        self.region
    }

    pub fn cuts(&self) -> &[Marker] {
        self.markers.list()
    }

    /// Cuts detection proposed before any fit.
    pub fn proposed(&self) -> usize {
        self.proposed
    }

    /// Propose the cuts again from the settings, keeping every cut the
    /// player placed or moved.
    pub fn redetect(&mut self) {
        let (start, end) = self.region;
        let proposal = match self.mode {
            ChopMode::Transient => {
                let slot = band_slot(self.band);
                let (pcm, band) = (&self.pcm, self.band);
                self.curves[slot]
                    .get_or_insert_with(|| OnsetCurve::analyse(pcm, start, end, band))
                    .pick(pcm, self.sensitivity)
            }
            ChopMode::Grid => grid::beats(&self.pcm, start, end, self.bars, self.division),
            ChopMode::Equal => grid::equal(&self.pcm, start, end, self.slices),
        };
        self.markers.propose(proposal);
        self.proposed = self.markers.len();
        if let (ChopMode::Transient, Some(count)) = (self.mode, self.fit) {
            self.markers.fit(count);
        }
        self.clamp_selected();
    }

    fn clamp_selected(&mut self) {
        self.selected = self.selected.min(self.markers.len().saturating_sub(1));
    }

    pub fn slices(&self) -> Vec<(u64, u64)> {
        self.markers.slices()
    }

    /// The slice under the cursor.
    pub fn selected_slice(&self) -> Option<(u64, u64)> {
        self.slices().get(self.selected).copied()
    }

    /// Step the cursor through the slices, stopping at both ends.
    pub fn select(&mut self, delta: i32) {
        let last = self.markers.len().saturating_sub(1) as i32;
        self.selected = (self.selected as i32 + delta).clamp(0, last.max(0)) as usize;
    }

    /// `a`: a cut of the player's own in the middle of the slice under the
    /// cursor, or at the region's start when there are no cuts yet. The
    /// cursor moves onto it, ready to be held and walked to the hit.
    pub fn add_cut(&mut self) -> bool {
        let at = match self.selected_slice() {
            Some((start, end)) => start + (end - start) / 2,
            None => self.region.0,
        };
        match self.markers.add(&self.pcm, at) {
            Some(index) => {
                self.selected = index;
                true
            }
            None => false,
        }
    }

    /// `d`: take the cut under the cursor away; its slice joins the one
    /// before.
    pub fn remove_cut(&mut self) -> bool {
        let removed = self.markers.remove(self.selected).is_some();
        if removed {
            self.selected = self.selected.saturating_sub(1);
            self.clamp_selected();
        }
        removed
    }

    /// Move the cut under the cursor, pinning it. Returns where it went.
    pub fn nudge_cut(&mut self, delta: i64, snap: bool) -> Option<u64> {
        self.markers.nudge(&self.pcm, self.selected, delta, snap)
    }

    /// The keys the slices will land on, first and last.
    pub fn keys(&self) -> Option<(usize, usize)> {
        let count = self.markers.len();
        (count > 0).then(|| (self.first, (self.first + count - 1).min(NUM_PADS - 1)))
    }

    /// The keys the slices will land on, in words: `C2`, or `C2–E2`.
    pub fn keys_label(&self) -> String {
        land::span_label(self.first, self.markers.len())
    }

    /// Start the landing where it fits on `state`: C2 when those keys are
    /// free, otherwise the first free run above it, otherwise the first
    /// free run anywhere. Where nothing fits it stays at C2, and the screen
    /// says why before the player asks.
    ///
    /// Asked once, as the screen opens. The keys are the player's to move
    /// after that, and a landing that walked about on its own every time a
    /// setting changed the count would be a landing nobody could aim.
    ///
    /// The reason it exists: a chop of a sound already on the bed — the
    /// slice of a break chopped before, say — would otherwise start on top
    /// of that sound and refuse on the first press, every time.
    pub fn place(&mut self, state: &SamplerState) {
        let count = self.markers.len().max(1);
        let home = SamplerState::pad_of_note(DEFAULT_FIRST_NOTE).unwrap_or(0);
        if let Some(first) = (home..NUM_PADS).chain(0..home).find(|&f| land::fits(state, f, count)) {
            self.first = first;
        }
    }

    /// Why this chop cannot land on `state` as it stands, or `None`.
    pub fn refusal(&self, state: &SamplerState) -> Option<String> {
        land::refusal(state, self.markers.len(), self.first)
    }

    /// Land it. See [`land::land`].
    pub fn land(&self, state: &mut SamplerState) -> Result<land::Landing, String> {
        land::land(state, &self.source, &self.slices(), self.first, self.feel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampler::chop::testkit::{boom_bap, frames_with, Drum, SR};
    use std::path::PathBuf;

    fn plan() -> ChopPlan {
        let pcm = Arc::new(boom_bap(SR).0);
        ChopPlan::new(LayerState::from_wav(PathBuf::from("break.wav"), pcm)).unwrap()
    }

    #[test]
    fn a_fresh_plan_cuts_at_the_big_hits_and_lands_from_c2() {
        let plan = plan();
        let (_, hits) = boom_bap(SR);
        let big = hits.iter().filter(|(_, d)| d.contains(&Drum::Kick) || d.contains(&Drum::Snare)).count();
        assert_eq!(plan.cuts().len(), big);
        assert_eq!(SamplerState::pad_label(plan.first), "C2");
        assert_eq!(plan.keys(), Some((plan.first, plan.first + big - 1)));
    }

    #[test]
    fn a_sound_with_no_audio_cannot_be_planned() {
        let mut layer = LayerState::from_wav(PathBuf::from("gone.wav"), Arc::new(boom_bap(SR).0));
        layer.pcm = None;
        assert!(ChopPlan::new(layer).is_none());
    }

    /// The source's trim is the chop's region: the count-in trimmed off is
    /// not chopped back on.
    #[test]
    fn the_chop_is_taken_from_the_trimmed_region() {
        let pcm = Arc::new(boom_bap(SR).0);
        let (_, hits) = boom_bap(SR);
        let mut layer = LayerState::from_wav(PathBuf::from("break.wav"), pcm);
        let snares = frames_with(&hits, Drum::Snare);
        layer.start_frame = snares[0] as u64 + 2_000;
        let plan = ChopPlan::new(layer).unwrap();
        assert!(plan.cuts().iter().all(|c| c.frame >= plan.region().0));
    }

    #[test]
    fn switching_the_band_to_low_cuts_at_the_kicks() {
        let mut plan = plan();
        plan.band = Band::Low;
        plan.redetect();
        let (_, hits) = boom_bap(SR);
        assert_eq!(plan.cuts().len(), frames_with(&hits, Drum::Kick).len());
    }

    #[test]
    fn a_fit_keeps_the_strongest_and_forgets_itself_when_lifted() {
        let mut plan = plan();
        let all = plan.cuts().len();
        plan.fit = Some(2);
        plan.redetect();
        assert_eq!(plan.cuts().len(), 2);
        assert_eq!(plan.proposed(), all, "the fit lost count of what it thinned");
        plan.fit = None;
        plan.redetect();
        assert_eq!(plan.cuts().len(), all, "lifting the fit did not bring the cuts back");
    }

    #[test]
    fn grid_and_equal_cut_the_region_evenly() {
        let mut plan = plan();
        plan.mode = ChopMode::Grid;
        plan.bars = 1;
        plan.division = Division::Sixteenth;
        plan.redetect();
        assert_eq!(plan.cuts().len(), 16);
        plan.mode = ChopMode::Equal;
        plan.slices = 5;
        plan.redetect();
        assert_eq!(plan.cuts().len(), 5);
        assert_eq!(plan.cuts()[0].frame, plan.region().0);
    }

    /// The player's hand-work outlives every change of setting.
    #[test]
    fn a_moved_cut_survives_a_change_of_band_and_of_mode() {
        let mut plan = plan();
        plan.select(1);
        let moved = plan.nudge_cut(480, true).unwrap();
        for (mode, band) in [(ChopMode::Transient, Band::High), (ChopMode::Equal, Band::High)] {
            plan.mode = mode;
            plan.band = band;
            plan.redetect();
            assert!(plan.cuts().iter().any(|c| c.frame == moved && c.pinned), "{mode:?} lost it");
        }
    }

    #[test]
    fn add_cuts_the_selected_slice_in_two_and_remove_joins_it_back() {
        let mut plan = plan();
        let before = plan.cuts().len();
        let (start, end) = plan.selected_slice().unwrap();
        assert!(plan.add_cut());
        assert_eq!(plan.cuts().len(), before + 1);
        let cut = plan.cuts()[plan.selected].frame;
        assert!(cut > start && cut < end && plan.cuts()[plan.selected].pinned);
        assert!(plan.remove_cut());
        assert_eq!(plan.cuts().len(), before);
        while plan.remove_cut() {}
        assert!(plan.cuts().is_empty() && plan.selected_slice().is_none());
        assert!(plan.add_cut(), "an empty plan could not be started by hand");
        assert_eq!(plan.cuts()[0].frame, plan.region().0);
    }

    #[test]
    fn the_cursor_stays_on_a_slice_that_exists() {
        let mut plan = plan();
        plan.select(100);
        assert_eq!(plan.selected, plan.cuts().len() - 1);
        plan.select(-100);
        assert_eq!(plan.selected, 0);
        plan.select(100);
        plan.fit = Some(1);
        plan.redetect();
        assert_eq!(plan.selected, 0, "a fit left the cursor on a slice it removed");
    }

    /// Chopping a slice that sits on C2 must not start on top of it.
    #[test]
    fn placing_starts_at_c2_and_steps_past_whatever_is_in_the_way() {
        let mut plan = plan();
        let count = plan.cuts().len();
        let mut state = SamplerState::new();
        plan.place(&state);
        assert_eq!(SamplerState::pad_label(plan.first), "C2");
        state.add_wav_layer(plan.first + 1, PathBuf::from("x.wav"), Arc::clone(plan.pcm())).unwrap();
        plan.place(&state);
        assert_eq!(plan.first, SamplerState::pad_of_note(DEFAULT_FIRST_NOTE).unwrap() + 2);
        assert!(plan.refusal(&state).is_none());
        // Nowhere free for the run: it stays where it was and says why.
        for pad in (0..NUM_PADS).step_by(count.max(2) - 1) {
            let _ = state.add_wav_layer(pad, PathBuf::from("x.wav"), Arc::clone(plan.pcm()));
        }
        plan.place(&state);
        assert!(plan.refusal(&state).is_some());
    }

    #[test]
    fn it_lands_what_it_shows_and_refuses_what_the_kit_will_not_take() {
        let plan = plan();
        let mut state = SamplerState::new();
        assert!(plan.refusal(&state).is_none());
        let landing = plan.land(&mut state).unwrap();
        assert_eq!(landing.count, plan.cuts().len());
        assert!(plan.refusal(&state).unwrap().contains("already hold sounds"));
    }
}
