export type DocumentState = {
  path: string | null;
  creationDirectory?: string | null;
  name: string;
  content: string;
  sizeBytes?: number;
  deleted?: boolean;
  dirty: boolean;
  open: boolean;
  revision: number;
};
