import { installServerSignalHandlers, startServer } from './index.js';
import { installRuntimeErrorLogging } from './runtime-logging.js';

installRuntimeErrorLogging('service');
installServerSignalHandlers(startServer());
