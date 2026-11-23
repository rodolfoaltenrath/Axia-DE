//! Sound: the PulseAudio native protocol (`pulse`) and Axia's client of pipewire-pulse (`snd`).
//! The C ABI of server/src/sound/{pulse,snd}.h lives here; the headers stay for the C callers
//! (axia-server's main loop and JS bridge, the media code).
pub mod pulse;
pub mod snd;
