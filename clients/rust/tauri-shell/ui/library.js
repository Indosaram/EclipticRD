/** DOM-free computer library; classic-script and CommonJS entry points. */
(function (root, factory) {
  const api = factory();
  root.LibraryModel = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})(typeof window !== 'undefined' ? window : globalThis, function () {
  'use strict';

  const FAVORITES_KEY = 'eclipticrd.favorites.v1';

  function normalizeHostKey(ip) {
    return ip.trim().toLowerCase();
  }

  function selectHosts(hosts, {
    query = '', availableOnly = false, favoritesOnly = false, favoriteIps = [],
  } = {}) {
    const needle = query.trim().toLowerCase();
    const favorites = new Set(favoriteIps.map(normalizeHostKey));
    return hosts.filter(host =>
      (!availableOnly || host.online === true) &&
      (!favoritesOnly || favorites.has(normalizeHostKey(host.ip))) &&
      (!needle || [host.name, host.ip, host.os].some(value =>
        typeof value === 'string' && value.toLowerCase().includes(needle))));
  }

  function normalizedIps(ips) {
    if (!Array.isArray(ips) || !ips.every(ip => typeof ip === 'string')) {
      throw new Error('Saved favorites must be an array of IP strings.');
    }
    return [...new Set(ips.map(normalizeHostKey))];
  }

  function createFavoritesStorage(storage) {
    return {
      load() {
        const value = storage.getItem(FAVORITES_KEY);
        return value === null ? [] : normalizedIps(JSON.parse(value));
      },
      save(ips) {
        storage.setItem(FAVORITES_KEY, JSON.stringify(normalizedIps(ips)));
      },
    };
  }

  function createLibrary({ invoke, storage, nativeAvailable }) {
    const preferences = createFavoritesStorage(storage);
    const listeners = new Set();
    let hosts = [], favoriteIps = [];
    let query = '', availableOnly = false, favoritesOnly = false;
    let status = nativeAvailable ? 'idle' : 'unavailable';
    let error = null, preferenceError = null, requestId = 0;
    try {
      favoriteIps = preferences.load();
    } catch (err) {
      preferenceError = 'Favorites could not be loaded. Choices are available for this session only. ' + displayError(err);
    }

    function snapshot() {
      return {
        hosts: hosts.slice(),
        visibleHosts: selectHosts(hosts, { query, availableOnly, favoritesOnly, favoriteIps }),
        query, availableOnly, favoritesOnly, favoriteIps: favoriteIps.slice(),
        status, error, preferenceError,
      };
    }

    function emit() {
      for (const listener of listeners) listener(snapshot());
    }

    async function refresh() {
      if (!nativeAvailable) return;
      const current = ++requestId;
      status = 'loading';
      error = null;
      emit();
      try {
        const result = await invoke('list_hosts');
        if (current !== requestId) return;
        if (!Array.isArray(result)) throw new Error('Computer list response was not an array.');
        hosts = result.slice();
        status = 'ready';
      } catch (err) {
        if (current !== requestId) return;
        status = 'error';
        error = displayError(err);
      }
      emit();
    }

    return {
      snapshot,
      subscribe(listener) {
        listeners.add(listener);
        return () => listeners.delete(listener);
      },
      refresh,
      setQuery(value) { query = value; emit(); },
      setAvailableOnly(value) { availableOnly = value; emit(); },
      setFavoritesOnly(value) { favoritesOnly = value; emit(); },
      clearFilters() { availableOnly = false; favoritesOnly = false; emit(); },
      toggleFavorite(ip) {
        const key = normalizeHostKey(ip);
        favoriteIps = favoriteIps.includes(key)
          ? favoriteIps.filter(value => value !== key) : [...favoriteIps, key];
        try {
          preferences.save(favoriteIps);
          preferenceError = null;
        } catch (err) {
          preferenceError = 'Favorites could not be saved. Choices are available for this session only. ' + displayError(err);
        }
        emit();
      },
    };
  }

  // Display data only: the page must render errors with textContent, never HTML.
  function displayError(error) {
    return error instanceof Error ? error.message : String(error);
  }

  return { normalizeHostKey, selectHosts, createFavoritesStorage, createLibrary };
});
