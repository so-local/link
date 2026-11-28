// Next 16.4.6's `next/experimental/testing/server` reads
// `globalThis.AsyncLocalStorage` when it loads, but only Next's own server
// start-up sets it, so the redirect tests' calls throw "AsyncLocalStorage
// accessed in runtime where it is not available". The tests set it first,
// as the server does, from Node's own module. Remove this once a Next
// release's testing entry point sets it itself.
import { AsyncLocalStorage } from "node:async_hooks";

Object.assign(globalThis, { AsyncLocalStorage });
