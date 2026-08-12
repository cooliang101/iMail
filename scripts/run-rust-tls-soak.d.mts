export interface RustTlsSoakArguments {
  durationSeconds: number;
  maximumGrowthMiB: number;
  reportPath: string;
}

export function parseArguments(values: string[]): RustTlsSoakArguments;
