// Development-only, read-only visual renderer. This entry is excluded from release builds.
import { mount } from 'svelte';
import App from './App.svelte';
import { createController } from './lib/controller';
import { createCodexController } from './lib/codex-controller';
import { codexCapability, codexDemoSnapshot } from './test/codex-fixtures';
import { snapshot, account } from './test/fixtures';
import './styles.css';
import './theme.css';
import { t } from './lib/i18n';

if (!import.meta.env.DEV) throw new Error(t('en', 'previewDevOnly'));
const demo = snapshot({ demo: true });
demo.accounts[0].email = 'studio@example.invalid';
demo.accounts[1].email = 'personal@example.invalid';
const research = account('research', {
  name: 'Research',
  email: 'research@example.invalid',
  isNext: false,
});
research.usage!.fiveHour.utilization = 92;
research.usage!.weeklyOverall = 84;
research.usage!.weeklyModel = 88;
research.usage!.scopedLimits[0].percent = 88;
demo.accounts.push(research);
demo.consumptionPlan = ['b', 'research', 'a'];

demo.settings.language =
  new URLSearchParams(location.search).get('lang') === 'ro' ? 'ro' : 'en';
const theme = new URLSearchParams(location.search).get('theme');
if (theme === 'dark' || theme === 'light') demo.settings.appearance = theme;
const controller = createController({
  call: async (command) => {
    if (command === 'get_snapshot') return demo;
    throw new Error(t(demo.settings.language, 'demoOnly'));
  },
  subscribe: async () => () => {},
});
// Codex preview states: ?codex=switching | signin | limited | setup | empty
const codexDemo = codexDemoSnapshot();
const codexScenario = new URLSearchParams(location.search).get('codex');
if (codexScenario === 'switching')
  codexDemo.switching = {
    targetId: 'codex-research',
    stage: 'restarting',
    startedAt: Math.floor(Date.now() / 1000) - 23,
  };
else if (codexScenario === 'signin') {
  Object.assign(codexDemo.accounts[2], {
    needsSignIn: true,
    quotaState: 'unavailable',
    error: 'signInRequired',
    switchable: codexCapability('signInRequired'),
  });
  codexDemo.nextId = null;
  codexDemo.order = [];
} else if (codexScenario === 'limited')
  Object.assign(codexDemo.accounts[0].quota!.limits[0].primary!, {
    usedPercent: 100,
    resetsAt: Math.floor(Date.now() / 1000) + 47 * 60,
  });
else if (codexScenario === 'setup')
  Object.assign(codexDemo, {
    availability: 'notInstalled',
    blockedReason: 'notInstalled',
    executableVersion: null,
    nextId: null,
    order: [],
  });
else if (codexScenario === 'empty')
  Object.assign(codexDemo, {
    accounts: [],
    selectedId: null,
    nextId: null,
    order: [],
  });
const codexController = createCodexController({
  call: async (command) => {
    if (command === 'get_codex_snapshot') return codexDemo;
    throw new Error('providerUnavailable');
  },
  subscribe: async () => () => {},
});
mount(App, {
  target: document.getElementById('app')!,
  props: { controller, codexController },
});
