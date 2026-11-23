import { luminance16 } from '../image/decode';

/** Longest side processed; larger video is block-averaged down to it. */
export const MAX_VIDEO_SIDE = 1920;
const MIN_SIDE = 64;
const SEEK_TIMEOUT_MS = 8000;

export interface CameraFrame {
  timeS: number;
  /** Frames the camera has delivered so far, when the browser reports it. */
  presented: number | null;
}

export interface VideoSource {
  /** Frame size after binning. */
  readonly width: number;
  readonly height: number;
  readonly binning: number;
  /** Length in seconds; null for a live camera. */
  readonly durationS: number | null;
  readonly live: boolean;
  /** Moves a file to the frame shown at `timeS`. */
  seek(timeS: number): Promise<void>;
  /** Waits for the next camera frame. */
  nextFrame(): Promise<CameraFrame>;
  /** The current frame as 16-bit linear luminance. */
  grab(): Uint16Array;
  close(): void;
}

function once(target: EventTarget, type: string, timeoutMs: number): Promise<void> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      target.removeEventListener(type, done);
      reject(new Error(`video did not report "${type}" within ${timeoutMs / 1000} s`));
    }, timeoutMs);
    const done = () => {
      clearTimeout(timer);
      resolve();
    };
    target.addEventListener(type, done, { once: true });
  });
}

function mediaError(video: HTMLVideoElement): Error {
  const detail = video.error?.message;
  return new Error(detail ? `the browser cannot decode this video (${detail})` : 'the browser cannot decode this video');
}

async function untilReady(video: HTMLVideoElement): Promise<void> {
  if (video.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA) {
    return;
  }
  await new Promise<void>((resolve, reject) => {
    video.addEventListener(
      'loadeddata',
      () => {
        resolve();
      },
      { once: true },
    );
    video.addEventListener(
      'error',
      () => {
        reject(mediaError(video));
      },
      { once: true },
    );
  });
}

/**
 * Recordings made by `MediaRecorder` carry no duration until the element
 * has been sent to their end once.
 */
async function resolveDuration(video: HTMLVideoElement): Promise<number> {
  if (Number.isFinite(video.duration)) {
    return video.duration;
  }
  video.currentTime = Number.MAX_SAFE_INTEGER;
  await once(video, 'seeked', SEEK_TIMEOUT_MS);
  const duration = Number.isFinite(video.duration) ? video.duration : video.currentTime;
  video.currentTime = 0;
  await once(video, 'seeked', SEEK_TIMEOUT_MS);
  return duration;
}

class ElementSource implements VideoSource {
  readonly width: number;
  readonly height: number;
  readonly binning: number;
  private readonly context: CanvasRenderingContext2D;

  constructor(
    private readonly video: HTMLVideoElement,
    readonly durationS: number | null,
    private readonly release: () => void,
  ) {
    const { videoWidth, videoHeight } = video;
    this.binning = Math.max(1, Math.ceil(Math.max(videoWidth, videoHeight) / MAX_VIDEO_SIDE));
    this.width = Math.floor(videoWidth / this.binning);
    this.height = Math.floor(videoHeight / this.binning);
    if (this.width < MIN_SIDE || this.height < MIN_SIDE) {
      throw new Error(`video is too small (${videoWidth} x ${videoHeight})`);
    }
    const canvas = document.createElement('canvas');
    canvas.width = videoWidth;
    canvas.height = videoHeight;
    const context = canvas.getContext('2d', { willReadFrequently: true });
    if (!context) {
      throw new Error('2D canvas is unavailable');
    }
    this.context = context;
  }

  get live(): boolean {
    return this.durationS === null;
  }

  async seek(timeS: number): Promise<void> {
    // A freshly loaded element has not painted its first frame until it is
    // sought, so the first request always seeks, by a hair when needed.
    const target = Math.abs(this.video.currentTime - timeS) < 1e-4 ? timeS + 1e-3 : timeS;
    this.video.currentTime = target;
    await once(this.video, 'seeked', SEEK_TIMEOUT_MS);
  }

  nextFrame(): Promise<CameraFrame> {
    return new Promise((resolve) => {
      this.video.requestVideoFrameCallback((_now, metadata) => {
        resolve({ timeS: metadata.mediaTime, presented: metadata.presentedFrames });
      });
    });
  }

  grab(): Uint16Array {
    const { videoWidth, videoHeight } = this.video;
    this.context.drawImage(this.video, 0, 0);
    const data = this.context.getImageData(0, 0, videoWidth, videoHeight);
    return luminance16(data.data, videoWidth, videoHeight, this.binning);
  }

  close(): void {
    this.video.pause();
    this.release();
    this.video.removeAttribute('src');
    this.video.srcObject = null;
    this.video.load();
  }
}

function element(): HTMLVideoElement {
  const video = document.createElement('video');
  video.muted = true;
  video.playsInline = true;
  video.preload = 'auto';
  return video;
}

export async function openVideoFile(file: Blob): Promise<VideoSource> {
  const video = element();
  const url = URL.createObjectURL(file);
  const release = () => {
    URL.revokeObjectURL(url);
  };
  try {
    video.src = url;
    await untilReady(video);
    const duration = await resolveDuration(video);
    return new ElementSource(video, duration, release);
  } catch (error) {
    release();
    throw error;
  }
}

export async function openCamera(): Promise<VideoSource> {
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: false,
    video: { facingMode: 'environment', width: { ideal: MAX_VIDEO_SIDE } },
  });
  const release = () => {
    stream.getTracks().forEach((track) => {
      track.stop();
    });
  };
  try {
    const video = element();
    video.srcObject = stream;
    await video.play();
    await untilReady(video);
    return new ElementSource(video, null, release);
  } catch (error) {
    release();
    throw error;
  }
}
