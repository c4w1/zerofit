// The UI side of the worker: a typed async RPC. `call("analyze", …)`
// returns a promise of exactly what the worker's handler returns.

import type { Method, Methods, Request, Response } from "./protocol";

let worker: Worker | null = null;
let nextId = 1;
const pending = new Map<number, { resolve: (v: unknown) => void; reject: (e: Error) => void }>();

function getWorker(): Worker {
  if (!worker) {
    worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
    worker.onmessage = (event: MessageEvent<Response>) => {
      const msg = event.data;
      const p = pending.get(msg.id);
      if (!p) return;
      pending.delete(msg.id);
      if (msg.ok) p.resolve(msg.result);
      else p.reject(new Error(msg.error));
    };
    worker.onerror = (event) => {
      for (const p of pending.values()) p.reject(new Error(event.message || "worker error"));
      pending.clear();
    };
  }
  return worker;
}

/**
 * Calls `method` in the worker. ArrayBuffers in `transfer` are moved to
 * the worker (they become empty here).
 */
export function call<M extends Method>(
  method: M,
  args: Parameters<Methods[M]>,
  transfer: Transferable[] = [],
): Promise<ReturnType<Methods[M]>> {
  const id = nextId++;
  const request: Request<M> = { id, method, args };
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve: resolve as (v: unknown) => void, reject });
    getWorker().postMessage(request, transfer);
  });
}
