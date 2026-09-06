// Browser regression harness. It mocks IPC and never changes the installed WARP client.
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';

const fixturePath = fileURLToPath(new URL('./fixtures/warp-ui.js', import.meta.url)).replaceAll('\\', '/');
const indexPath = new URL('../src/index.html', import.meta.url);
const server = await createServer({
  server: { port: Number(process.env.WARP_PREVIEW_PORT || 1420) },
  plugins: [{
    name: 'warp-regression-preview',
    configureServer(server) {
      server.middlewares.use(async (req, res, next) => {
        if (!req.url?.startsWith('/warp-preview.html')) return next();
        try {
          const html = (await readFile(indexPath, 'utf8')).replace('src="/main.js"', `src="/@fs/${fixturePath}"`);
          res.setHeader('Content-Type', 'text/html');
          res.end(await server.transformIndexHtml(req.url, html));
        } catch (error) { next(error); }
      });
    },
  }],
});
await server.listen();
console.log(`WARP UI regression checks: http://127.0.0.1:${server.config.server.port}/warp-preview.html`);
console.log('Results appear below the home page after about 6 seconds. Optional: ?lang=en&theme=light&proxy=1');
process.once('SIGINT', async () => { await server.close(); process.exit(0); });
