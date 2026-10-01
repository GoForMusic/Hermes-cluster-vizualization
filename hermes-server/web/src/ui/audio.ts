// The alarm sound. Browsers only allow audio after a user gesture, e.g. toggling the sound switch once.
let ctx: AudioContext | undefined;

export function beep(): void {
  try {
    ctx ??= new AudioContext();
    void ctx.resume();
    const audio = ctx;
    [0, 0.22, 0.44].forEach((t, i) => {
      const o = audio.createOscillator();
      const g = audio.createGain();
      o.type = 'square';
      o.frequency.value = i === 1 ? 660 : 880;
      const t0 = audio.currentTime + t;
      g.gain.setValueAtTime(0.05, t0);
      g.gain.exponentialRampToValueAtTime(0.0001, t0 + 0.18);
      o.connect(g).connect(audio.destination);
      o.start(t0);
      o.stop(t0 + 0.2);
    });
  } catch { /* audio is not available */ }
}
