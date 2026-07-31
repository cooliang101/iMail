import { installServerSignalHandlers, startServer } from './index.js';

installServerSignalHandlers(startServer());
