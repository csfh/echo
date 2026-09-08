/** Browser bindings for the `echo_wasm` check host (`just wasm`). */

import { ECHO_WASM_REV } from "./echo-wasm-rev";

export type CheckDiagnostic = {
  severity: "error" | "warning" | "note" | string;
  code?: string;
  message: string;
  path: string;
  line: number;
  column: number;
  end_line: number;
  end_column: number;
};

export type CheckResult = {
  ok: boolean;
  diagnostics: CheckDiagnostic[];
};

export type FormatResult = {
  ok: boolean;
  text?: string;
  diagnostics: CheckDiagnostic[];
};

export type RunResult = {
  ok: boolean;
  printed?: string;
  host_error?: string;
  diagnostics: CheckDiagnostic[];
};

export type EchoCheckApi = {
  check: (source: string) => CheckResult;
  format: (source: string) => FormatResult;
  run: (source: string) => RunResult;
  stdFileCount: number;
};

type WasmModule = {
  default: (opts?: { module_or_path?: string | URL }) => Promise<unknown>;
  check: (source: string) => string;
  format: (source: string) => string;
  playgroundRun?: (source: string) => string;
  run?: (source: string) => string;
  stdFileCount: () => number;
};

let pending: Promise<EchoCheckApi> | null = null;

export class EchoWasmMissingError extends Error {
  constructor() {
    super("The in-browser checker is not built. From a checkout run `just wasm`, then reload.");
    this.name = "EchoWasmMissingError";
  }
}

export function loadEchoCheck(): Promise<EchoCheckApi> {
  pending ??= importEchoWasm();
  return pending;
}

async function importEchoWasm(): Promise<EchoCheckApi> {
  let mod: WasmModule;
  const jsUrl = `/echo-wasm/echo_wasm.js?v=${ECHO_WASM_REV}`;
  const wasmUrl = `/echo-wasm/echo_wasm_bg.wasm?v=${ECHO_WASM_REV}`;
  try {
    mod = (await import(/* @vite-ignore */ jsUrl)) as WasmModule;
  } catch (error) {
    throw rewriteLoadError(error);
  }

  try {
    await mod.default({ module_or_path: new URL(wasmUrl, window.location.origin) });
  } catch (error) {
    throw rewriteLoadError(error);
  }

  const runExport = pickRunExport(mod);
  if (runExport == null) {
    throw new EchoWasmMissingError();
  }

  return {
    check(source: string) {
      return JSON.parse(mod.check(source)) as CheckResult;
    },
    format(source: string) {
      const result = JSON.parse(mod.format(source)) as FormatResult;
      result.diagnostics ??= [];
      return result;
    },
    run(source: string) {
      const result = JSON.parse(runExport(source)) as RunResult;
      result.diagnostics ??= [];
      return result;
    },
    stdFileCount: mod.stdFileCount(),
  };
}

function pickRunExport(mod: WasmModule): ((source: string) => string) | null {
  if (typeof mod.playgroundRun === "function") {
    return mod.playgroundRun.bind(mod);
  }
  if (typeof mod.run === "function") {
    return mod.run.bind(mod);
  }
  return null;
}

function rewriteLoadError(error: unknown): Error {
  if (error instanceof EchoWasmMissingError) {
    return error;
  }
  console.error("echo_wasm load failed", error);
  return new EchoWasmMissingError();
}
