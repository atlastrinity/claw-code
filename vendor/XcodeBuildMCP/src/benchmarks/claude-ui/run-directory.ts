import { access, readdir } from 'node:fs/promises';
import path from 'node:path';
import { main } from './harness.ts';

async function directoryExists(directory: string): Promise<boolean> {
  try {
    await access(directory);
    return true;
  } catch {
    return false;
  }
}

export async function suitePaths(directory: string): Promise<string[]> {
  if (!(await directoryExists(directory))) return [];
  const entries = await readdir(directory, { withFileTypes: true });
  return entries
    .filter(
      (entry) => entry.isFile() && (entry.name.endsWith('.yml') || entry.name.endsWith('.yaml')),
    )
    .map((entry) => path.join(directory, entry.name))
    .sort();
}

type RunSuite = (args: string[]) => Promise<number>;

export async function runDirectory(args: string[], runSuite: RunSuite = main): Promise<number> {
  const directory = args[0];
  const maybeLabel = args[1];
  const label = maybeLabel && !maybeLabel.startsWith('-') ? maybeLabel : directory;
  const forwardedArgs = maybeLabel && !maybeLabel.startsWith('-') ? args.slice(2) : args.slice(1);
  if (!directory) {
    console.error('Usage: run-directory.ts <suite-directory> [label] [benchmark args...]');
    return 1;
  }

  const suites = await suitePaths(directory);
  if (suites.length === 0) {
    console.error(`No ${label} Claude UI benchmark suites found in ${directory}`);
    return 1;
  }

  let finalExitCode = 0;
  for (const suite of suites) {
    const exitCode = await runSuite(['--suite', suite, ...forwardedArgs]);
    if (exitCode !== 0 && finalExitCode === 0) {
      finalExitCode = exitCode;
    }
  }
  return finalExitCode;
}
