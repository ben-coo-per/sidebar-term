// Watermark back-pressure for one Session's pty stream (docs/research/pty.md, "Flow control";
// https://xtermjs.org/docs/guides/flowcontrol/). Pure: no xterm, no IPC, so it is unit-tested.
//
// Usage per chunk of pty output:
//   flow.written(bytes.length);
//   term.write(bytes, () => flow.processed(bytes.length));
// `onPause` fires once when the bytes handed to xterm but not yet parsed rise above `high`;
// `onResume` fires once when they fall below `low` again. The gap between the two is the
// hysteresis that stops the Session flapping between paused and running.

/** Starting points from the research: xterm parses 5-35 MB/s, so 200 KB is ~6-40 ms of work. */
export const DEFAULT_HIGH_WATERMARK = 200_000;
export const DEFAULT_LOW_WATERMARK = 50_000;

export interface FlowControlOptions {
  /** Pause once more than this many bytes are pending. */
  high?: number;
  /** Resume once fewer than this many bytes are pending. Must be below `high`. */
  low?: number;
  onPause: () => void;
  onResume: () => void;
}

export class FlowController {
  readonly high: number;
  readonly low: number;
  private readonly onPause: () => void;
  private readonly onResume: () => void;
  private _pending = 0;
  private _paused = false;

  constructor(opts: FlowControlOptions) {
    this.high = opts.high ?? DEFAULT_HIGH_WATERMARK;
    this.low = opts.low ?? DEFAULT_LOW_WATERMARK;
    if (!(this.low >= 0 && this.low < this.high)) {
      throw new RangeError(`FlowController needs 0 <= low < high (got low=${this.low}, high=${this.high})`);
    }
    this.onPause = opts.onPause;
    this.onResume = opts.onResume;
  }

  /** Bytes handed to the terminal whose write callback has not fired yet. */
  get pending(): number {
    return this._pending;
  }

  /** True between an `onPause` and the matching `onResume`. */
  get paused(): boolean {
    return this._paused;
  }

  /** Record `n` bytes passed to `terminal.write`. */
  written(n: number): void {
    this._pending += n;
    if (!this._paused && this._pending > this.high) {
      this._paused = true;
      this.onPause();
    }
  }

  /** Record that the write callback for `n` bytes fired (xterm parsed them). */
  processed(n: number): void {
    this._pending = Math.max(0, this._pending - n);
    if (this._paused && this._pending < this.low) {
      this._paused = false;
      this.onResume();
    }
  }
}
