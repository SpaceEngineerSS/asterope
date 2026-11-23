import type { EngineDefaults, Job, Request, Response } from './protocol';

export type JobResponse = Exclude<Response, { type: 'ready' } | { type: 'fatal' }>;

function transferables(job: Job): Transferable[] {
  if (job.kind === 'video') {
    return [job.pixels.buffer];
  }
  if (job.kind === 'image' && job.pixels) {
    return [job.pixels.buffer];
  }
  if (job.kind === 'fits') {
    return [job.bytes];
  }
  return [];
}

function isFinal(response: JobResponse): boolean {
  switch (response.type) {
    case 'trials':
    case 'centroid':
    case 'stress':
      return response.done;
    case 'progress':
      return false;
    default:
      return true;
  }
}

export class EngineClient {
  readonly ready: Promise<EngineDefaults>;
  private readonly worker: Worker;
  private readonly handlers = new Map<number, (response: JobResponse) => void>();
  private nextId = 1;

  constructor(epochYear: number) {
    this.worker = new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' });
    this.ready = new Promise((resolve, reject) => {
      this.worker.onmessage = (event: MessageEvent<Response>) => {
        const response = event.data;
        if (response.type === 'ready') {
          resolve(response.defaults);
        } else if (response.type === 'fatal') {
          reject(new Error(response.message));
        } else {
          const handler = this.handlers.get(response.id);
          if (isFinal(response)) {
            this.handlers.delete(response.id);
          }
          handler?.(response);
        }
      };
      this.worker.onerror = (event) => {
        reject(new Error(event.message));
      };
    });
    this.send({ type: 'init', epochYear });
  }

  submit(job: Job, onResponse: (response: JobResponse) => void): number {
    const id = this.nextId;
    this.nextId += 1;
    this.handlers.set(id, onResponse);
    this.send({ type: 'job', id, job }, transferables(job));
    return id;
  }

  cancel(id: number): void {
    this.send({ type: 'cancel', id });
  }

  private send(request: Request, transfer: Transferable[] = []): void {
    this.worker.postMessage(request, transfer);
  }
}

/**
 * Runs at most one job at a time and keeps only the newest job submitted
 * while busy, so a dragged slider never queues stale frames. A running job
 * that has been superseded is asked to stop and its output is discarded.
 */
export class LatestWins {
  private current: number | null = null;
  private pending: Job | null = null;

  constructor(
    private readonly client: EngineClient,
    private readonly onResponse: (response: JobResponse) => void,
    private readonly onIdle: () => void,
  ) {}

  push(job: Job): void {
    if (this.current !== null) {
      this.pending = job;
      this.client.cancel(this.current);
      return;
    }
    this.start(job);
  }

  /** Asks the running job to finish early; its (partial) result is still delivered. */
  stop(): void {
    if (this.current !== null) {
      this.client.cancel(this.current);
    }
  }

  private start(job: Job): void {
    this.current = this.client.submit(job, (response) => {
      const superseded = this.pending !== null;
      if (!superseded) {
        this.onResponse(response);
      }
      if (response.type === 'progress') {
        return;
      }
      const next = this.pending;
      this.pending = null;
      this.current = null;
      if (next) {
        this.start(next);
      } else {
        this.onIdle();
      }
    });
  }
}
