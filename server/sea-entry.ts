import { applyPackagedServiceArguments, parsePackagedServiceArguments } from './sea-arguments.js';

const input = parsePackagedServiceArguments(process.argv.slice(2));
applyPackagedServiceArguments(input);

if (input.syncWorker) {
  const { runSyncWorkerProcess } = require('./sync/worker.js') as typeof import('./sync/worker.js');
  runSyncWorkerProcess();
} else {
  const { installServerSignalHandlers, startServer } = require('./index.js') as typeof import('./index.js');
  installServerSignalHandlers(startServer());
}
