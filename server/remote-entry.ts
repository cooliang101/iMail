import path from 'node:path';

process.env.NODE_ENV ||= 'production';
process.env.HOST ||= '0.0.0.0';
process.env.IMAIL_WEB_DIST ||= path.resolve('dist');

void import('./index.js').then(({ installServerSignalHandlers, startServer }) => {
  installServerSignalHandlers(startServer());
});
