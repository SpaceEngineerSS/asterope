const BITS_PER_SECOND = 16_000_000;

const LINEAR_TO_SRGB = Uint8Array.from({ length: 4096 }, (_, index) => {
  const c = index / 4095;
  const encoded = c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055;
  return Math.round(255 * encoded);
});

function mimeType(): string {
  const preferred = 'video/webm;codecs=vp9';
  return MediaRecorder.isTypeSupported(preferred) ? preferred : 'video/webm';
}

interface Capture {
  track: MediaStreamTrack;
  /** Asks for the canvas as it is now to be taken as a frame. */
  requestFrame: () => void;
  recorder: MediaRecorder;
}

type FrameSource = { requestFrame?: () => void };

/**
 * Records sensor frames as a WebM video. Frames are encoded with the sRGB
 * transfer curve, which the video reader inverts, and are stamped with the
 * wall-clock time at which they were added.
 */
export class FrameRecorder {
  private readonly canvas: HTMLCanvasElement;
  private readonly context: CanvasRenderingContext2D;
  private readonly image: ImageData;
  private readonly chunks: Blob[] = [];
  private capture: Capture | null = null;
  frames = 0;

  constructor(width: number, height: number) {
    if (typeof MediaRecorder === 'undefined') {
      throw new Error('this browser cannot record video');
    }
    this.canvas = document.createElement('canvas');
    this.canvas.width = width;
    this.canvas.height = height;
    const context = this.canvas.getContext('2d');
    if (!context) {
      throw new Error('2D canvas is unavailable');
    }
    this.context = context;
    this.image = context.createImageData(width, height);
  }

  matches(width: number, height: number): boolean {
    return this.canvas.width === width && this.canvas.height === height;
  }

  /** The stream is opened once the canvas holds a frame, so the video does not begin on an empty one. */
  private open(): Capture {
    // Chromium and WebKit take frames through the track, Firefox through
    // the stream.
    const stream = this.canvas.captureStream(0);
    const [track] = stream.getVideoTracks();
    const source = [track as FrameSource | undefined, stream as FrameSource].find((candidate) => typeof candidate?.requestFrame === 'function');
    if (!track || !source) {
      throw new Error('this browser cannot record a canvas');
    }
    const recorder = new MediaRecorder(stream, {
      mimeType: mimeType(),
      videoBitsPerSecond: BITS_PER_SECOND,
    });
    recorder.ondataavailable = (event) => {
      this.chunks.push(event.data);
    };
    recorder.start();
    return { track, requestFrame: () => source.requestFrame?.(), recorder };
  }

  /** Adds a frame whose brightest representable value is `fullScale`. */
  add(pixels: Uint16Array, fullScale: number): void {
    const rgba = this.image.data;
    const scale = 4095 / Math.max(fullScale, 1);
    for (let i = 0; i < pixels.length; i += 1) {
      const value = LINEAR_TO_SRGB[Math.min(4095, Math.round((pixels[i] ?? 0) * scale))] ?? 0;
      const o = i * 4;
      rgba[o] = value;
      rgba[o + 1] = value;
      rgba[o + 2] = value;
      rgba[o + 3] = 255;
    }
    this.context.putImageData(this.image, 0, 0);
    this.capture ??= this.open();
    this.capture.requestFrame();
    this.frames += 1;
  }

  finish(): Promise<Blob> {
    const capture = this.capture;
    if (!capture) {
      return Promise.resolve(new Blob([], { type: 'video/webm' }));
    }
    return new Promise((resolve) => {
      capture.recorder.onstop = () => {
        capture.track.stop();
        resolve(new Blob(this.chunks, { type: 'video/webm' }));
      };
      capture.recorder.stop();
    });
  }
}
