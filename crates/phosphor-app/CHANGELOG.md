# Changelog

What changed, newest first. The application reads this file to show a "what's
new" card the first time you run a version you have not seen. Each section is a
`## X.Y.Z — Title` header, a blank line, and a short prose summary — the same
words as the release notes, without the install line, which the app supplies.

## 0.3.86 — The app tells you what changed, and when there's more

Phosphor now keeps you current on its own. The first time you run a version you have not seen, a small card sums up what changed — read straight from a changelog that ships inside the binary, so it works offline and is always right for the version in front of you. And once a day, in the background, it quietly checks crates.io; when a newer build is out it shows one dismissible line telling you to run the install command to get it. The check is pure-Rust and never blocks startup, offline is silent, and `PHOSPHOR_NO_UPDATE_CHECK=1` turns it off entirely.

## 0.3.85 — A mono trigger, and the save cursor on your own session

When you open the save picker with a session already loaded, the cursor now lands on that session's own row, so reaching for the list finds your project under the cursor instead of a neighbour you might overwrite by reflex — and Enter still saves straight over it with no question. Each sampler pad also gains a third trigger mode beside one-shot and gate: mono, which plays like a one-shot but is cut by the next note played on any pad. Only one mono voice ever sounds and it is always the newest, giving last-note-wins monophony across the whole instrument — exactly what a monosynth bass line or an 808 glide wants.

## 0.3.84 — Saving over what you had is one key again

A save flow got tangled by the sampler's own bookkeeping: a session's `.samples` sidecar folder sorted to the top of the save picker, so Enter walked into it and dropped a second copy of the session inside. The sidecar folder is no longer listed in the open or save pickers, and the save picker now offers the open session's own name in the name line — so Enter saves straight over your file with no overwrite question, while typing the first character replaces the offer for a spin-off saved beside it. Overwrite still asks before clobbering a different existing file.

## 0.3.83 — Time cannot bend silently any more

A player reported everything playing at half speed while every number on the screen stayed right; the culprit was a mono output device, on which the stereo engine advanced the transport at exactly half rate. The audio stream now bridges any device to the engine's stereo — mono folds the pair, wider devices get the front pair — so time is permanently out of the device's hands. A build that cannot keep up stretches time the same way, so the DSP crates are optimized even in dev builds and the engine counts its own missed deadlines into a top-bar warning. New always-on tests measure the metronome and a song's notes in rendered audio at both common device rates, so the clock can never quietly regress.

## 0.3.82 — The pad shows its hand

The sampler stops keeping secrets. A pad recorded from an instrument now names its source right on its panel, the filled-pad list marks which pads carry a source, and pressing `i` on such a pad reopens a picker that returns you to it with patch and panel intact. Under the parameters sits a compact waveform of the selected sound, the trimmed region bright and the cut ends dim, and in the trim strip `w` hugs the sound — one press pulls both markers to the audible edges. Building it fixed a long-standing bug: the automatic tail-trim on recorded takes had never actually worked, so takes always kept their dead air.

## 0.3.81 — One home for every session, and a save that shows you where

A player saved a project and could not find it again, because saving and opening could disagree about where projects live depending on the launch directory. Now every session lives in one home (`~/.phosphor/sessions`, or `%APPDATA%\phosphor\sessions` on Windows) and both pickers answer from the same rule. Saving grew the same little file tree opening has: walk to any folder, type the name into the name line, and Enter saves there — with a y/n ask before an existing project is overwritten. Projects saved under the old rule remain openable.

## 0.3.80 — Dial the sound you are about to record

Press `i` on a pad and pick an instrument, and now while source mode is on you can Tab over to the `[inst]` panel and edit that instrument's real patch — every knob in its own units, heard live, because the synth genuinely is in the track's slot. Dial the sound, Tab back, record, and the take renders through what you made; land it as a phrase and the hosted instrument adopts the same panel. This also closes a quiet hazard where the panel kept showing the sampler's own two knobs while the slot held a synth, so turning "level" could silently change the patch.

## 0.3.79 — Nobody types a path any more

Space+O now opens a file picker: your projects folder listed right there, `j`/`k` to walk it, Enter to open, and from the first letter you type the list narrows live (at which point the arrows take over and the footer says so). Saving asks for a name, not a path — type it and the project lands in the folder the picker looks at. On the sampler, `a` browses your samples folder the same way, with your session's own recorded takes pinned one Enter away, and the old typed-path prompt is a keypress away behind `/`. This release also carries a platform-by-platform Quick Start for Windows, macOS and Linux.

## 0.3.78 — The sampler, finished

`cycle` joins the pad panel, so five snare takes stacked on one pad play in turn instead of machine-gunning the same one. Phrases now remember the sample rate they were captured at and play at pitch on any device, wide zone edits ship the whole keyboard as a single command, and the pad and zone lists carry a memory line naming what your kit holds. Under the hood every clamp and constant the engine and interface once spelled separately lives in one place, and the workspace's warning count fell from forty-seven to eleven. This closes the sampler arc that began at v0.3.72: eighty-eight pads, external WAVs, resampling any built-in instrument, audio and phrase layers, chromatic zones, trim, round robin, and sessions that carry all of it.

## 0.3.77 — Phrases, and four QA scalps

In source mode, `p` flips what `r` lands: audio renders your take to a buffer, while phrase keeps it as notes and replays it live through one child instrument hosted inside the sampler — a pad can carry a whole chord stab at a fraction of the memory. Phrases obey everything a sampled layer does — polyphony cuts them, choke groups stop them, gates release them mid-flight — and sessions carry them inline as notes with the child beside them. Four fixes from an adversarial QA sweep land too: a confirm now freezes the pad it names, a session that fails to open no longer strands the sampler silent, root-learn cannot survive a trip through source mode, and a full pad still opens its load prompt so a typed path can never run as key commands.

## 0.3.76 — Keys mode

`K` on the pad map flips the bed from pads to keys: zones, a span of keys sharing one sound transposed from a root. `w` throws a zone across the whole bed, `o` takes the octave, `s` splits a zone at the caret, and Enter locks the span brace so `h`/`l` and `H`/`L` walk its edges. The root arrives three ways — recorded from a single-pitch take, read from the filename, or played — and overlapping zones retune their lent layers by the distance between roots so everything sounds the pitch you play. Under the hood a zone materializes onto its keys as reference-counted handles, so an 88-key zone costs refcounts, not copies.

## 0.3.75 — Record the machine into itself

On any pad, `i` picks a source — the Rhodes, the DX7, any synth in the house — and the track slips into source mode where your keys play that synth live. `r` arms, you perform, `r` lands it: with the transport stopped the take is free and auto-trimmed to just before the first sound; with it rolling the take is whole bars that loop clean on the seam. The render is offline and deterministic and runs through the track's MIDI effects, so a take through the arp sounds like what you heard. Takes are saved as float WAVs in a sidecar folder beside the session, and Save As carries the recordings along to the new home.

## 0.3.74 — The trim strip

Press `t` on a loaded pad layer and its waveform crosses the whole pane — trimmed region bright, cut ends dim, markers on the edges. `h` and `l` walk the start point, `H` and `L` the end, and `j`/`k` go deeper down a ladder of nudge units from bar to single sample, figured against the file's own sample rate. Zero-crossing snap is on by default, `r` plays the region in reverse, every nudge of the start auditions from the new start so you find the attack by ear, and a whole nudge run is one undo step. Underneath is a new audition path that sounds exactly one layer on dedicated voices, with loop seams made of two crossing edge fades so a dialled-in loop plays clean.

## 0.3.73 — The pad map

A sampler track now opens on a pads tab: the full 88-key bed drawn across the screen with a caret over the current pad, filled pads in the track's colour carrying their layer count, and a knob panel in the same grammar as everything else. Free, `h` and `l` walk the bed and `H`/`L` jump an octave; `j`/`k` pick a control, Enter holds it, and held, `h`/`l` turn it. Every edit records undo at the right grain — knob sweeps fold to one step, removals stand alone and can always be taken back — and undoing a deleted sampler track now brings its whole kit back. Under the hood the keyboard band and knob panel became shared widgets that the practice room and step sequencer draw through too.

## 0.3.72 — Type a path, hear the pad

The Sampler stops being a phosphor synth wearing a name tag. Underneath is a new engine: 88 pads, each stacking up to eight layers of shared PCM, with one-shot and gate triggers, per-pad polyphony, choke groups, pitch over ±48 semitones, ADSR, non-destructive trim, reverse, and the invisible manners that separate a sampler from a click generator — cubic interpolation, edge fades, native-rate compensation and a proven allocation-free audio path. Pick Sampler from the instrument menu, play a key to choose its pad, press `a` and type a path, and the take lands as a layer. Sessions store paths rather than audio, and a file that moved keeps its pad and its settings with a note in the status bar instead of silently losing the kit.
