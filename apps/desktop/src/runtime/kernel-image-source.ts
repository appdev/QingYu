import type {
  KernelImageSource,
  KernelResourceBody,
  KernelResourceSnapshot,
} from "@markra/app/runtime";

export interface DesktopObjectUrlApi {
  readonly createObjectURL: (blob: Blob) => string;
  readonly revokeObjectURL: (url: string) => unknown;
}

export function createDesktopKernelImageSource(
  objectUrls: DesktopObjectUrlApi = URL,
): KernelImageSource {
  const active = new Set<string>();
  return Object.freeze({
    materialize: async (
      _resource: KernelResourceSnapshot,
      open: () => Promise<KernelResourceBody>,
    ) => {
      const body = await open();
      const source = objectUrls.createObjectURL(body.body);
      active.add(source);
      return source;
    },
    release: (source: string) => {
      if (!active.delete(source)) return undefined;
      objectUrls.revokeObjectURL(source);
      return undefined;
    },
  });
}
