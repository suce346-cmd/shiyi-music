export declare const NODE_RANGES: number[][];
export declare const ENGINES_NODE: string;
export declare function nodeVersionOk(version: string): boolean;
export declare function iCloudManagedAncestor(dir: string, hasFileProviderAttr: (p: string) => boolean, home: string): string | null;
export declare function runGuard(o: {
  cwd: string; version: string; platform: string; env: Record<string, string | undefined>; home: string; hasAttr: (p: string) => boolean;
}): string[];
