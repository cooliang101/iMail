import path from 'node:path';
import { installRuntimeErrorLogging } from './runtime-logging.js';

process.env.NODE_ENV ||= 'production';
process.env.HOST ||= '0.0.0.0';
process.env.IMAIL_WEB_DIST ||= path.resolve('dist');

installRuntimeErrorLogging('service');
void import('./index.js').then(({ installServerSignalHandlers, startServer }) => installServerSignalHandlers(startServer()));
