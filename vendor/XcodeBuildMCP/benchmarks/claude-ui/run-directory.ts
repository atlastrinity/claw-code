#!/usr/bin/env tsx
import { runDirectory } from '../../src/benchmarks/claude-ui/run-directory.ts';

runDirectory(process.argv.slice(2))
  .then((exitCode) => {
    process.exitCode = exitCode;
  })
  .catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  });
