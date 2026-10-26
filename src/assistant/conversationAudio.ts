/** One WebAudio output queue for every voice provider, beside the microphone's
 * WebRTC echo cancellation. Epochs also guard async decoding after Stop. */
export class ConversationAudio {
  private generation = 0;
  private epoch: number | null = null;
  private nextAt = 0;
  private sources = new Set<AudioBufferSourceNode>();
  private decoding: Promise<void> = Promise.resolve();
  private gain: GainNode;
  /** Taps the voice before the volume, so the orb moves with the assistant's
   *  speech however loud the user has set it. */
  private analyser: AnalyserNode | null = null;
  private samples: Float32Array | null = null;

  constructor(
    private context: AudioContext,
    private onPlaying: (playing: boolean) => void,
    volume = 1,
  ) {
    this.gain = context.createGain();
    this.gain.connect(context.destination);
    if (typeof context.createAnalyser === "function") {
      const analyser = context.createAnalyser();
      analyser.fftSize = 512;
      this.analyser = analyser;
      this.samples = new Float32Array(analyser.fftSize);
    }
    this.setVolume(volume);
  }

  /** The loudness of what is playing right now, as an RMS (0 when silent). */
  level(): number {
    const analyser = this.analyser;
    const samples = this.samples;
    if (!analyser || !samples || this.sources.size === 0) return 0;
    analyser.getFloatTimeDomainData(samples);
    let sum = 0;
    for (const sample of samples) sum += sample * sample;
    return Math.sqrt(sum / samples.length);
  }

  /**
   * Apply a new playback volume, including to audio already queued.
   *
   * The gain node is shared by every source, so this takes effect mid-reply:
   * moving the voice-volume slider during a call should be audible in that call,
   * not in the next one.
   */
  setVolume(volume: number) {
    const level = Math.max(0, Math.min(1, volume));
    if (this.gain.gain.value === level) return;
    this.gain.gain.value = level;
  }

  begin(epoch: number) {
    if (this.epoch === epoch) return;
    this.stop();
    this.epoch = epoch;
  }

  enqueue(bytes: ArrayBuffer, epoch: number): Promise<void> {
    if (epoch !== this.epoch) return Promise.resolve();
    const generation = this.generation;
    const decoded = this.decoding.then(async () => {
      if (generation !== this.generation) return;
      const buffer = await this.context.decodeAudioData(bytes);
      if (generation !== this.generation || epoch !== this.epoch) return;
      const source = this.context.createBufferSource();
      source.buffer = buffer;
      source.connect(this.gain);
      if (this.analyser) source.connect(this.analyser);
      this.sources.add(source);
      source.onended = () => {
        source.disconnect();
        this.sources.delete(source);
        if (generation === this.generation && this.sources.size === 0)
          this.onPlaying(false);
      };
      const at = Math.max(this.context.currentTime + 0.015, this.nextAt);
      this.nextAt = at + buffer.duration;
      source.start(at);
      this.onPlaying(true);
    });
    // A failed decode must not poison every subsequent utterance.
    this.decoding = decoded.catch(() => {});
    return decoded;
  }

  stop() {
    this.generation++;
    this.epoch = null;
    for (const source of this.sources) {
      source.onended = null;
      source.stop();
      source.disconnect();
    }
    this.sources.clear();
    this.nextAt = 0;
    this.decoding = Promise.resolve();
    this.onPlaying(false);
  }
}
