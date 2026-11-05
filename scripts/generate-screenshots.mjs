#!/usr/bin/env node
import { readFile, mkdir, copyFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { dirname, resolve, join } from 'node:path';
import { createServer } from 'vite';
import { chromium } from 'playwright-core';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const rootDir = resolve(__dirname, '..');
const screenshotsDir = join(rootDir, 'screenshots');
const websiteScreenshotsDir = join(rootDir, 'docs', 'screenshots');
const indexPath = join(rootDir, 'src', 'index.html');

await mkdir(screenshotsDir, { recursive: true });

const mockPath = fileURLToPath(new URL('./screenshot-mock.js', import.meta.url)).replaceAll('\\', '/');

console.log('Starting Vite server for screenshots...');
const serverPort = 1428;
const server = await createServer({
  configFile: resolve(rootDir, 'vite.config.js'),
  server: { port: serverPort, strictPort: false },
  plugins: [
    {
      name: 'screenshot-fixture-middleware',
      configureServer(server) {
        server.middlewares.use(async (req, res, next) => {
          if (!req.url?.startsWith('/screenshot.html')) return next();
          try {
            const rawHtml = (await readFile(indexPath, 'utf8')).replace('<head>', '<head><link rel="icon" href="data:,">');
            const transformed = rawHtml.replace(
              '<script type="module" src="/main.js"></script>',
              `<script type="module" src="/@fs/${mockPath}"></script>\n    <script type="module" src="/main.js"></script>`
            );
            res.setHeader('Content-Type', 'text/html');
            res.end(await server.transformIndexHtml(req.url, transformed));
          } catch (error) {
            next(error);
          }
        });
      },
    },
  ],
});

// Browser plugin not available; use the repository's Playwright/Edge workflow.
let browser;
try {
await server.listen();
const baseUrl = `http://127.0.0.1:${server.config.server.port}`;
console.log(`Vite server ready at ${baseUrl}`);

console.log('Launching browser via playwright-core...');
try {
  browser = await chromium.launch({
    channel: 'msedge',
    headless: true,
  });
} catch (e) {
  console.log('Could not launch msedge, trying default chrome...');
  browser = await chromium.launch({
    channel: 'chrome',
    headless: true,
  });
}

const context = await browser.newContext({
  viewport: { width: 1100, height: 980 },
  deviceScaleFactor: 2,
});

const page = await context.newPage();
const pageErrors = [];
page.on('response', response => {
  if (response.status() >= 400) pageErrors.push(`${response.status()} ${response.url()}`);
});
page.on('console', (msg) => {
  if (msg.type() === 'error') pageErrors.push(msg.text());
});
page.on('pageerror', (err) => pageErrors.push(String(err)));

async function waitForAppReady() {
  await page.waitForFunction(() => !document.documentElement.classList.contains('app-loading'));
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(300);
}

const screens = [
  {
    name: 'home',
    title: 'Главная страница (Home)',
    action: async () => {
      await page.goto(`${baseUrl}/screenshot.html`);
      await waitForAppReady();
      await page.evaluate(async () => {
        const { showSection } = await import('/features/navigation.js');
        showSection('home');
      });
      await page.waitForTimeout(250);
    },
  },
  {
    name: 'telegram',
    title: 'Telegram прокси (TG WS Proxy)',
    action: async () => {
      await page.evaluate(async () => {
        const { showSection } = await import('/features/navigation.js');
        showSection('telegram');
      });
      await page.waitForSelector('#section-telegram:not(.hidden)');
      await page.waitForTimeout(250);
    },
  },
  {
    name: 'warp',
    title: 'Настройки Cloudflare WARP (WARP Settings)',
    action: async () => {
      await page.evaluate(async () => {
        const { showSection } = await import('/features/navigation.js');
        showSection('warp-settings');
      });
      await page.waitForSelector('#section-warp-settings:not(.hidden)');
      await page.waitForTimeout(250);
    },
  },
  {
    name: 'sites',
    title: 'Списки сайтов (Site Lists)',
    action: async () => {
      await page.evaluate(async () => {
        const { showSection } = await import('/features/navigation.js');
        const { loadUserLists } = await import('/features/user-lists.js');
        showSection('sites');
        document.getElementById('section-zapret-settings')?.scrollTo(0, 0);
        await loadUserLists();
      });
      await page.waitForSelector('#zapret-settings-sites:not(.hidden)');
      await page.waitForTimeout(250);
    },
  },
  {
    name: 'ips',
    title: 'Настройки IP и фильтров (IP Settings)',
    action: async () => {
      await page.evaluate(async () => {
        const { showSection } = await import('/features/navigation.js');
        const { loadUserLists } = await import('/features/user-lists.js');
        showSection('ips');
        document.getElementById('section-zapret-settings')?.scrollTo(0, 0);
        await loadUserLists();
      });
      await page.waitForSelector('#zapret-settings-ips:not(.hidden)');
      await page.waitForTimeout(250);
    },
  },
  {
    name: 'diagnostics',
    title: 'Диагностика (Diagnostics)',
    action: async () => {
      await page.evaluate(async () => {
        const { showSection } = await import('/features/navigation.js');
        showSection('diagnostics');
        document.getElementById('section-zapret-settings')?.scrollTo(0, 0);
      });
      await page.waitForSelector('#zapret-settings-diagnostics:not(.hidden)');
      await page.locator('#run-diagnostics-btn').click();
      await page.locator('#diagnostics-show-all-btn').waitFor({ state: 'visible' });
      await page.locator('#diagnostics-show-all-btn').click();
    },
  },
  {
    name: 'traffic',
    title: 'Монитор трафика (Traffic Monitor)',
    action: async () => {
      await page.evaluate(async () => {
        const { showSection } = await import('/features/navigation.js');
        showSection('traffic');
      });
      await page.waitForSelector('#section-traffic:not(.hidden)');
      await page.locator('#traffic-toggle-btn').click();
      await page.waitForFunction(() => Number(document.getElementById('traffic-connection-count').textContent) > 0);
      await page.waitForTimeout(1100);
    },
  },
  {
    name: 'settings',
    title: 'Настройки (Settings)',
    action: async () => {
      await page.evaluate(async () => {
        const { showSection } = await import('/features/navigation.js');
        showSection('settings');
      });
      await page.waitForSelector('#section-settings:not(.hidden)');
      await page.waitForTimeout(250);
    },
  },
  {
    name: 'automations',
    title: 'Автоматизации (Application Rules)',
    action: async () => {
      await page.evaluate(async () => {
        const { showSection } = await import('/features/navigation.js');
        showSection('automations');
      });
      await page.waitForSelector('#section-automations:not(.hidden)');
      await page.waitForTimeout(250);
    },
  },
  {
    name: 'status',
    title: 'Окно проверки статуса ZAPRET (Status Modal)',
    action: async () => {
      await page.evaluate(async () => {
        const { showSection } = await import('/features/navigation.js');
        showSection('home');
        document.getElementById('check-status-btn')?.click();
      });
      await page.waitForSelector('#status-modal:not(.hidden)');
      await page.waitForTimeout(400);
    },
    cleanup: async () => {
      await page.evaluate(() => {
        document.getElementById('status-modal')?.classList.add('hidden');
      });
    },
  },
  {
    name: 'setup',
    title: 'Мастер первоначальной настройки (Setup Wizard)',
    action: async () => {
      await page.goto(`${baseUrl}/screenshot.html?setup=1`);
      await waitForAppReady();
      await page.waitForSelector('.setup-dialog');
      await page.waitForTimeout(300);
    },
  },
];

console.log(`\nGenerating ${screens.length} screenshots (2x scale, 2200x1960)...`);
const results = [];

for (const screen of screens) {
  process.stdout.write(`Capturing [${screen.name}.png] - ${screen.title}... `);
  await screen.action();
  await page.evaluate(() => {
    window.scrollTo(0, 0);
    document.documentElement.scrollTop = 0;
    document.body.scrollTop = 0;
    document.getElementById('sections-host')?.scrollTo(0, 0);
  });
  if (screen.name !== 'setup') {
    await page.locator('#app-header').scrollIntoViewIfNeeded();
  }
  await page.mouse.move(1095, 975);
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  if (pageErrors.length) throw new Error(`Screenshot ${screen.name} has runtime errors: ${pageErrors.join('\n')}`);
  if (await page.locator('vite-error-overlay').count()) throw new Error(`Vite error overlay in ${screen.name}`);

  const filePath = join(screenshotsDir, `${screen.name}.png`);
  const screenshot = await page.screenshot({
    omitBackground: true,
    animations: 'disabled',
  });
  const existing = await readFile(filePath).catch(error => {
    if (error.code !== 'ENOENT') throw error;
    return null;
  });
  // Keep unchanged images untouched, including files open in a preview window.
  if (!existing?.equals(screenshot)) await writeFile(filePath, screenshot);

  if (screen.cleanup) {
    await screen.cleanup();
  }

  results.push({ name: `${screen.name}.png`, path: filePath });
  console.log('✓ Done');
}

console.log('\nValidating generated PNG files (transparency & dimensions)...');
for (const res of results) {
  const buf = await readFile(res.path);
  const dataUri = `data:image/png;base64,${buf.toString('base64')}`;

  const check = await page.evaluate(async (src) => {
    return new Promise((resolve) => {
      const img = new Image();
      img.onload = () => {
        const canvas = document.createElement('canvas');
        canvas.width = img.width;
        canvas.height = img.height;
        const ctx = canvas.getContext('2d');
        ctx.drawImage(img, 0, 0);

        const w = img.width;
        const h = img.height;
        const getAlpha = (x, y) => ctx.getImageData(x, y, 1, 1).data[3];
        const header = ctx.getImageData(40, 20, 660, 120).data;
        let headerPixels = 0;
        for (let i = 0; i < header.length; i += 4) {
          if (header[i + 3] > 200 && Math.max(header[i], header[i + 1], header[i + 2]) > 125) headerPixels++;
        }

        resolve({
          width: w,
          height: h,
          tl: getAlpha(0, 0),
          tr: getAlpha(w - 1, 0),
          bl: getAlpha(0, h - 1),
          br: getAlpha(w - 1, h - 1),
          center: getAlpha(Math.floor(w / 2), Math.floor(h / 2)),
          headerPixels,
        });
      };
      img.src = src;
    });
  }, dataUri);

  const cornersTransparent = check.tl === 0 && check.tr === 0 && check.bl === 0 && check.br === 0;
  const insideOpaque = check.center === 255;
  const correctSize = check.width === 2200 && check.height === 1960;

  console.log(`  ✓ ${res.name}: ${check.width}x${check.height}px | corners alpha=[${check.tl}, ${check.tr}, ${check.bl}, ${check.br}], center=${check.center} | ${cornersTransparent && insideOpaque && correctSize ? 'VALID' : 'INVALID'}`);

  if (!cornersTransparent) {
    throw new Error(`Corners are not transparent in ${res.name} (TL=${check.tl}, TR=${check.tr}, BL=${check.bl}, BR=${check.br})`);
  }
  if (!correctSize) {
    throw new Error(`Unexpected dimensions for ${res.name}: expected 2200x1960, got ${check.width}x${check.height}`);
  }
  if (!insideOpaque) throw new Error(`Screenshot center is transparent in ${res.name}`);
  if (check.headerPixels < 100) throw new Error(`Window header is missing from ${res.name}`);
}

await mkdir(websiteScreenshotsDir, { recursive: true });
for (const result of results) {
  await copyFile(result.path, join(websiteScreenshotsDir, result.name));
}
await copyFile(join(screenshotsDir, 'home.png'), join(rootDir, 'docs', 'assets', 'app-screenshot.png'));

console.log(`\nAll ${results.length} screenshots generated and synchronized with docs/screenshots/.`);
} finally {
  await browser?.close();
  await server.close();
}
