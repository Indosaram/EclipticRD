import { test, expect } from 'bun:test';
import { openPage, hosts } from './page-harness.mjs';

test('Computers page renders sidebar, This Computer card, and nav with aria-current="page"', async () => {
  const page = await openPage({ hosts });
  try {
    await page.evaluate(`
      window.fixture.until(() => document.querySelectorAll('#host-grid [data-host-id]').length === ${hosts.length})
    `);

    const result = await page.evaluate(`({
      hasSidebar: !!document.querySelector('aside'),
      sidebarHeader: document.querySelector('aside header span')?.textContent,
      navComputersCurrent: document.getElementById('nav-computers')?.getAttribute('aria-current'),
      navFavoritesCurrent: document.getElementById('nav-favorites')?.getAttribute('aria-current'),
      hasThisComputer: !!document.getElementById('this-computer-card'),
      thisComputerTitle: document.getElementById('this-computer-title')?.textContent,
      thisComputerBadge: document.getElementById('this-computer-badge')?.textContent,
      thisComputerIp: document.getElementById('this-computer-ip')?.textContent,
      thisComputerPin: document.getElementById('this-computer-pin')?.textContent,
      toggleBtnText: document.getElementById('btn-host-toggle')?.textContent,
      copyBtnText: document.getElementById('btn-host-copy')?.textContent,
    })`);

    expect(result.hasSidebar).toBe(true);
    expect(result.sidebarHeader).toBe('MahoRD');
    expect(result.navComputersCurrent).toBe('page');
    expect(result.navFavoritesCurrent).toBe('false');

    expect(result.hasThisComputer).toBe(true);
    expect(result.thisComputerTitle).toBe('This Computer (Host)');
    expect(result.thisComputerBadge).toBe('READY');
    expect(result.thisComputerIp).toContain('127.0.0.1');
    expect(result.thisComputerPin).toBe('87654321');
    expect(result.toggleBtnText).toBe('Pause Sharing');
    expect(result.copyBtnText).toBe('Copy Info');
  } finally {
    await page.close();
  }
}, 20000);

test('host grid renders one card per stubbed host with real metadata and roles', async () => {
  const page = await openPage({ hosts });
  try {
    await page.evaluate(`
      window.fixture.until(() => document.querySelectorAll('#host-grid [data-host-id]').length === ${hosts.length})
    `);

    const gridInfo = await page.evaluate(`({
      heading: document.getElementById('library-heading')?.textContent,
      count: document.getElementById('library-count')?.textContent,
      gridRole: document.getElementById('host-grid')?.getAttribute('role'),
      cards: [...document.querySelectorAll('#host-grid [data-host-id]')].map(el => ({
        id: el.dataset.hostId,
        availability: el.dataset.availability,
        role: el.getAttribute('role'),
        favoritePressed: el.querySelector('[data-action="favorite"]')?.getAttribute('aria-pressed'),
        connectDisabled: el.querySelector('[data-action="connect"]')?.disabled,
        connectText: el.querySelector('[data-action="connect"]')?.textContent,
      })),
      calls: window.fixture.calls.map(c => c.cmd),
    })`);

    expect(gridInfo.heading).toContain('Listed computers');
    expect(gridInfo.count).toBe(String(hosts.length));
    expect(gridInfo.gridRole).toBe('list');
    expect(gridInfo.cards.length).toBe(hosts.length);

    for (let i = 0; i < hosts.length; i++) {
      const host = hosts[i];
      const card = gridInfo.cards[i];
      expect(card.id).toBe(host.id);
      expect(card.role).toBe('listitem');
      expect(card.availability).toBe(host.online ? 'available' : 'offline');
      expect(card.favoritePressed).toBe('false');
      expect(card.connectText).toBe('Connect');
      if (host.online) {
        expect(card.connectDisabled).toBe(false);
      } else {
        expect(card.connectDisabled).toBe(true);
      }
    }

    expect(gridInfo.calls).toContain('get_host_status');
    expect(gridInfo.calls).toContain('list_pairings');
    expect(gridInfo.calls).toContain('list_hosts');
  } finally {
    await page.close();
  }
}, 20000);

test('filter controls expose aria-pressed and filter the host grid', async () => {
  const page = await openPage({ hosts });
  try {
    await page.evaluate(`
      window.fixture.until(() => document.querySelectorAll('#host-grid [data-host-id]').length === ${hosts.length})
    `);

    const initialFilters = await page.evaluate(`({
      all: document.getElementById('filter-all')?.getAttribute('aria-pressed'),
      available: document.getElementById('filter-available')?.getAttribute('aria-pressed'),
      favorites: document.getElementById('filter-favorites')?.getAttribute('aria-pressed'),
      count: document.querySelectorAll('#host-grid [data-host-id]').length,
    })`);

    expect(initialFilters.all).toBe('true');
    expect(initialFilters.available).toBe('false');
    expect(initialFilters.favorites).toBe('false');
    expect(initialFilters.count).toBe(5);

    await page.evaluate(`
      window.fixture.until(
        () => document.querySelectorAll('#host-grid [data-host-id]').length === 4,
        () => document.getElementById('filter-available').click()
      )
    `);

    const availableFilters = await page.evaluate(`({
      all: document.getElementById('filter-all')?.getAttribute('aria-pressed'),
      available: document.getElementById('filter-available')?.getAttribute('aria-pressed'),
      favorites: document.getElementById('filter-favorites')?.getAttribute('aria-pressed'),
      count: document.querySelectorAll('#host-grid [data-host-id]').length,
      visibleIds: [...document.querySelectorAll('#host-grid [data-host-id]')].map(el => el.dataset.hostId),
    })`);

    expect(availableFilters.all).toBe('false');
    expect(availableFilters.available).toBe('true');
    expect(availableFilters.favorites).toBe('false');
    expect(availableFilters.count).toBe(4);
    expect(availableFilters.visibleIds).not.toContain('c');

    await page.evaluate(`
      window.fixture.until(
        () => document.querySelectorAll('#host-grid [data-host-id]').length === 5,
        () => document.getElementById('filter-all').click()
      )
    `);

    const resetFilters = await page.evaluate(`({
      all: document.getElementById('filter-all')?.getAttribute('aria-pressed'),
      available: document.getElementById('filter-available')?.getAttribute('aria-pressed'),
      count: document.querySelectorAll('#host-grid [data-host-id]').length,
    })`);

    expect(resetFilters.all).toBe('true');
    expect(resetFilters.available).toBe('false');
    expect(resetFilters.count).toBe(5);
  } finally {
    await page.close();
  }
}, 20000);

test('nav switches between Computers and Favorites views', async () => {
  const page = await openPage({ hosts });
  try {
    await page.evaluate(`
      window.fixture.until(() => document.querySelectorAll('#host-grid [data-host-id]').length === ${hosts.length})
    `);

    await page.evaluate(`
      window.fixture.until(
        () => document.getElementById('nav-favorites')?.getAttribute('aria-current') === 'page',
        () => document.getElementById('nav-favorites').click()
      )
    `);

    const favoritesView = await page.evaluate(`({
      navComputersCurrent: document.getElementById('nav-computers')?.getAttribute('aria-current'),
      navFavoritesCurrent: document.getElementById('nav-favorites')?.getAttribute('aria-current'),
      hasThisComputer: !!document.getElementById('this-computer-card'),
      statusMessage: document.getElementById('library-status')?.textContent,
    })`);

    expect(favoritesView.navComputersCurrent).toBe('false');
    expect(favoritesView.navFavoritesCurrent).toBe('page');
    expect(favoritesView.hasThisComputer).toBe(false);
    expect(favoritesView.statusMessage).toContain('No favorites here');

    await page.evaluate(`
      window.fixture.until(
        () => document.querySelectorAll('#host-grid [data-host-id]').length === 5,
        () => document.getElementById('nav-computers').click()
      )
    `);

    const computersView = await page.evaluate(`({
      navComputersCurrent: document.getElementById('nav-computers')?.getAttribute('aria-current'),
      navFavoritesCurrent: document.getElementById('nav-favorites')?.getAttribute('aria-current'),
      hasThisComputer: !!document.getElementById('this-computer-card'),
      count: document.querySelectorAll('#host-grid [data-host-id]').length,
    })`);

    expect(computersView.navComputersCurrent).toBe('page');
    expect(computersView.navFavoritesCurrent).toBe('false');
    expect(computersView.hasThisComputer).toBe(true);
    expect(computersView.count).toBe(5);
  } finally {
    await page.close();
  }
}, 20000);

test('host sharing toggle invokes backend commands and updates badge/button', async () => {
  const page = await openPage({ hosts });
  try {
    await page.evaluate(`
      window.fixture.until(() => document.getElementById('btn-host-toggle')?.textContent === 'Pause Sharing')
    `);

    await page.evaluate(`
      window.fixture.command('stop_host', () => document.getElementById('btn-host-toggle').click())
    `);
    await page.evaluate(`
      window.fixture.until(() => document.getElementById('btn-host-toggle')?.textContent === 'Resume Sharing')
    `);

    const paused = await page.evaluate(`({
      badge: document.getElementById('this-computer-badge')?.textContent,
      btnText: document.getElementById('btn-host-toggle')?.textContent,
    })`);
    expect(paused.badge).toBe('UNAVAILABLE');
    expect(paused.btnText).toBe('Resume Sharing');

    await page.evaluate(`
      window.fixture.command('start_host', () => document.getElementById('btn-host-toggle').click())
    `);
    await page.evaluate(`
      window.fixture.until(() => document.getElementById('btn-host-toggle')?.textContent === 'Pause Sharing')
    `);

    const resumed = await page.evaluate(`({
      badge: document.getElementById('this-computer-badge')?.textContent,
      btnText: document.getElementById('btn-host-toggle')?.textContent,
    })`);
    expect(resumed.badge).toBe('READY');
    expect(resumed.btnText).toBe('Pause Sharing');
  } finally {
    await page.close();
  }
}, 20000);

test('saved credentials section renders cards and supports forget action', async () => {
  const samplePairing = {
    id: 'saved-desk-1',
    hostName: 'Studio Workstation',
    addedAtUnixMs: 1726000000000,
    lastEndpoint: { host: '192.168.1.100', tcpPort: 19730, udpPort: 19731 },
  };

  const page = await openPage({ hosts, pairings: [samplePairing] });
  try {
    await page.evaluate(`
      window.fixture.until(() => document.querySelectorAll('[data-pairing-id]').length === 1)
    `);

    const pairingInfo = await page.evaluate(`({
      count: document.getElementById('saved-pairings-count')?.textContent,
      cardId: document.querySelector('[data-pairing-id]')?.dataset.pairingId,
      name: document.querySelector('[data-pairing-id] h3')?.textContent,
      text: document.querySelector('[data-pairing-id]')?.textContent,
    })`);

    expect(pairingInfo.count).toBe('1');
    expect(pairingInfo.cardId).toBe('saved-desk-1');
    expect(pairingInfo.name).toBe('Studio Workstation');
    expect(pairingInfo.text).toContain('192.168.1.100:19730');

    await page.evaluate(`
      window.fixture.command('forget_pairing', () => {
        document.querySelector('[data-action="forget-saved"]').click();
      })
    `);

    await page.evaluate(`
      window.fixture.until(() => !!document.getElementById('saved-pairings-empty'))
    `);

    const emptyText = await page.evaluate(`document.getElementById('saved-pairings-empty')?.textContent`);
    expect(emptyText).toContain('No saved credentials yet');
  } finally {
    await page.close();
  }
}, 20000);

test('host search filters grid and clear button restores inventory', async () => {
  const page = await openPage({ hosts });
  try {
    await page.evaluate(`
      window.fixture.until(() => document.querySelectorAll('#host-grid [data-host-id]').length === ${hosts.length})
    `);

    await page.evaluate(`
      window.fixture.until(
        () => document.querySelectorAll('#host-grid [data-host-id]').length === 3,
        () => window.fixture.type('#host-search', 'Linux')
      )
    `);

    const filtered = await page.evaluate(`({
      count: document.querySelectorAll('#host-grid [data-host-id]').length,
      ids: [...document.querySelectorAll('#host-grid [data-host-id]')].map(e => e.dataset.hostId),
    })`);

    expect(filtered.count).toBe(3);
    expect(filtered.ids).toEqual(['b', 'c', 'e']);

    await page.evaluate(`
      window.fixture.until(
        () => document.querySelectorAll('#host-grid [data-host-id]').length === ${hosts.length},
        () => document.getElementById('btn-clear-search').click()
      )
    `);

    const restoredCount = await page.evaluate(`document.querySelectorAll('#host-grid [data-host-id]').length`);
    expect(restoredCount).toBe(hosts.length);
  } finally {
    await page.close();
  }
}, 20000);

test('direct connection form validates input and submits connect command', async () => {
  const page = await openPage({ hosts });
  try {
    await page.evaluate(`
      window.fixture.until(() => document.querySelectorAll('#host-grid [data-host-id]').length === ${hosts.length})
    `);

    // Submitting empty host shows error
    await page.evaluate(`
      (() => {
        document.getElementById('direct-form').requestSubmit();
      })()
    `);
    const emptyErr = await page.evaluate(`document.getElementById('direct-error')?.textContent`);
    expect(emptyErr).toContain('Enter a host address');

    // Submitting invalid PIN (not 8 digits) shows error
    await page.evaluate(`
      (() => {
        window.fixture.type('#direct-ip', 'direct.host.test');
        window.fixture.type('#direct-pin', '123');
        document.getElementById('direct-form').requestSubmit();
      })()
    `);
    const pinErr = await page.evaluate(`document.getElementById('direct-error')?.textContent`);
    expect(pinErr).toContain('eight ASCII digits');

    // Submitting valid PIN triggers connect IPC call
    const connectCall = await page.evaluate(`
      window.fixture.command('connect', () => {
        window.fixture.type('#direct-pin', '12345678');
        document.getElementById('direct-form').requestSubmit();
      })
    `);

    expect(connectCall.args.host).toBe('direct.host.test');
    expect(connectCall.args.pin).toBe('12345678');
  } finally {
    await page.close();
  }
}, 20000);

test('page without native Tauri bridge gracefully indicates desktop controls unavailable', async () => {
  const page = await openPage({ noBridge: true });
  try {
    await page.evaluate(`
      new Promise((resolve) => {
        const check = () => {
          const msg = document.getElementById('library-status')?.textContent;
          if (msg && msg.includes('unavailable')) resolve();
          else setTimeout(check, 50);
        };
        check();
      })
    `);

    const status = await page.evaluate(`({
      statusText: document.getElementById('library-status')?.textContent,
      refreshDisabled: document.getElementById('btn-refresh')?.disabled,
      toggleDisabled: document.getElementById('btn-host-toggle')?.disabled,
      cardsCount: document.querySelectorAll('#host-grid [data-host-id]').length,
    })`);

    expect(status.statusText).toContain('Desktop connection controls are unavailable in this browser.');
    expect(status.refreshDisabled).toBe(true);
    expect(status.toggleDisabled).toBe(true);
    expect(status.cardsCount).toBe(0);
  } finally {
    await page.close();
  }
}, 20000);
