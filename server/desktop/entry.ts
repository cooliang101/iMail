import { startServer, installServerSignalHandlers } from '../index.js';

const server = startServer();
installServerSignalHandlers(server);
