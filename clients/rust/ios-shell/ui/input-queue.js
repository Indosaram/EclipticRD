(function (root, factory) {
  const api = factory();
  root.InputQueue = api;
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api;
  }
})(typeof window !== 'undefined' ? window : globalThis, function () {
  'use strict';

  function createInputQueue(options) {
    const invoke = options && typeof options.invoke === 'function'
      ? options.invoke
      : async () => {};
    const isCurrent = options && typeof options.isCurrent === 'function'
      ? options.isCurrent
      : () => true;
    const getGeneration = options && typeof options.getGeneration === 'function'
      ? options.getGeneration
      : () => 0;

    let chain = Promise.resolve();

    function enqueue(command, args, generation) {
      const capturedGen = generation !== undefined ? generation : getGeneration();

      const task = async () => {
        if (!isCurrent(capturedGen)) {
          return { dropped: true, command, args, generation: capturedGen };
        }
        try {
          const result = await invoke(command, args);
          return { dropped: false, result };
        } catch (err) {
          return { dropped: false, error: err };
        }
      };

      const resultPromise = chain.then(task, task);
      chain = resultPromise.catch(() => {});
      return resultPromise;
    }

    function clear() {
      chain = Promise.resolve();
    }

    return {
      enqueue,
      clear
    };
  }

  return {
    createInputQueue
  };
});
