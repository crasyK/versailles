"use strict";
(() => {
  // scripts/bar-shims/core.js
  function callerArgs(args) {
    const caller = typeof window !== "undefined" && window.__VERSAILLES_WIDGET_ID__ || void 0;
    if (!caller) return args;
    return Object.assign({ caller }, args || {});
  }
  function sdk() {
    const v = window.versailles;
    if (v && typeof v.invoke === "function") return v;
    return null;
  }
  function invoke(cmd, args) {
    const v = sdk();
    if (v) return v.invoke(cmd, callerArgs(args));
    const t = window.__TAURI__;
    if (t?.core?.invoke) return t.core.invoke(cmd, callerArgs(args));
    return Promise.reject(new Error("Versailles API not available in this window"));
  }

  // scripts/bar-shims/event.js
  function sdk2() {
    const v = window.versailles;
    if (v && typeof v.listen === "function") return v;
    return null;
  }
  function listen(event, handler) {
    const v = sdk2();
    if (v) return v.listen(event, (payload) => handler({ payload }));
    const t = window.__TAURI__;
    if (t?.event?.listen) return t.event.listen(event, handler);
    return Promise.reject(new Error("Versailles event API not available"));
  }

  // scripts/bar-shims/dpi.js
  var LogicalSize = class {
    constructor(width, height) {
      this.width = width;
      this.height = height;
      this.type = "Logical";
    }
  };
  var PhysicalPosition = class {
    constructor(x, y) {
      this.x = x;
      this.y = y;
      this.type = "Physical";
    }
  };

  // scripts/bar-shims/window.js
  function invoke2(cmd, args) {
    const v = window.versailles;
    if (v && typeof v.invoke === "function") return v.invoke(cmd, args);
    const t = window.__TAURI__;
    if (t?.core?.invoke) return t.core.invoke(cmd, args);
    return Promise.reject(new Error("Versailles API not available"));
  }
  function label() {
    return window.__TAURI_INTERNALS__?.metadata?.currentWindow?.label || "widget-action-bar";
  }
  function ipcSize(size) {
    const kind = size?.type === "Physical" ? "Physical" : "Logical";
    return { [kind]: { width: size.width, height: size.height } };
  }
  function ipcPos(pos) {
    const kind = pos?.type === "Physical" ? "Physical" : "Logical";
    return { [kind]: { x: pos.x, y: pos.y } };
  }
  function getCurrentWindow() {
    const l = label();
    return {
      label: l,
      async setSize(size) {
        return invoke2("plugin:window|set_size", { label: l, value: ipcSize(size) });
      },
      async setPosition(position) {
        return invoke2("plugin:window|set_position", { label: l, value: ipcPos(position) });
      },
      async currentMonitor() {
        return invoke2("plugin:window|current_monitor");
      },
      async isFocused() {
        return invoke2("plugin:window|is_focused", { label: l });
      },
      async onFocusChanged(handler) {
        const v = window.versailles;
        if (!v || typeof v.listen !== "function") return () => {
        };
        const offFocus = await v.listen("tauri://focus", () => handler({ payload: true }));
        const offBlur = await v.listen("tauri://blur", () => handler({ payload: false }));
        return () => {
          try {
            offFocus();
          } catch {
          }
          try {
            offBlur();
          } catch {
          }
        };
      }
    };
  }
  async function currentMonitor() {
    return getCurrentWindow().currentMonitor();
  }

  // scripts/bar-shims/xterm.js
  var Terminal = window.Terminal;

  // scripts/bar-shims/fit.js
  var FitAddon = window.FitAddon?.FitAddon || window.FitAddon;

  // scripts/bar-shims/web-links.js
  var WebLinksAddon = window.WebLinksAddon?.WebLinksAddon || window.WebLinksAddon;

  // scripts/bar-shims/search.js
  var SearchAddon = window.SearchAddon?.SearchAddon || window.SearchAddon;

  // src/engine/spawnable-config.ts
  var DEFAULT_OPTS = {
    blurDismissMs: 280,
    suggestionLimit: 12,
    compact: false,
    launchTick: false,
    searchHf: "https://huggingface.co/models?search={q}",
    timeAwareDefaults: true,
    autoDismissLaunch: true
  };
  function spawnableId() {
    const w = window;
    return w.__VERSAILLES_SPAWNABLE__?.id?.trim() || w.__VERSAILLES_WIDGET_ID__?.trim() || "action-bar";
  }
  function blockSpawnableOpts(id) {
    const block = window.__VERSAILLES_BLOCK__;
    const spawnables = block?.spawnables;
    if (!spawnables) return {};
    const key = id.trim().toLowerCase();
    return spawnables[key]?.opts ?? spawnables[id]?.opts ?? {};
  }
  function mergeOpts(host, block) {
    return {
      blurDismissMs: host?.blurDismissMs ?? block.blurDismissMs ?? DEFAULT_OPTS.blurDismissMs,
      suggestionLimit: host?.suggestionLimit ?? block.suggestionLimit ?? DEFAULT_OPTS.suggestionLimit,
      compact: host?.compact ?? block.compact ?? DEFAULT_OPTS.compact,
      launchTick: host?.launchTick ?? block.launchTick ?? DEFAULT_OPTS.launchTick,
      searchHf: host?.searchHf ?? block.searchHf ?? DEFAULT_OPTS.searchHf,
      timeAwareDefaults: host?.timeAwareDefaults ?? block.timeAwareDefaults ?? DEFAULT_OPTS.timeAwareDefaults,
      autoDismissLaunch: host?.autoDismissLaunch ?? block.autoDismissLaunch ?? DEFAULT_OPTS.autoDismissLaunch
    };
  }
  async function loadSpawnableEngineContext() {
    const id = spawnableId();
    const blockOpts = blockSpawnableOpts(id);
    try {
      const ctx = await invoke("get_spawnable_engine_context", { id });
      return {
        id: ctx.id || id,
        dismissOnBlur: ctx.dismissOnBlur,
        opts: mergeOpts(ctx.opts, blockOpts)
      };
    } catch {
      return {
        id,
        dismissOnBlur: false,
        opts: mergeOpts(void 0, blockOpts)
      };
    }
  }
  async function loadEngineRuntime(engineId) {
    try {
      return await invoke("get_engine_runtime", { engineId });
    } catch {
      return { recents: [], pins: [] };
    }
  }
  async function pushRecent(engineId, presetId) {
    if (!presetId.trim()) return;
    try {
      await invoke("patch_engine_runtime", {
        engineId,
        patch: { pushRecent: presetId.trim().toLowerCase() }
      });
    } catch {
    }
  }
  async function togglePin(engineId, presetId) {
    try {
      return await invoke("patch_engine_runtime", {
        engineId,
        patch: { togglePin: presetId.trim().toLowerCase() }
      });
    } catch {
      return loadEngineRuntime(engineId);
    }
  }
  async function saveLastTermSeed(engineId, seed) {
    try {
      await invoke("patch_engine_runtime", {
        engineId,
        patch: { lastTermSeed: seed }
      });
    } catch {
    }
  }

  // node_modules/fuse.js/dist/fuse.mjs
  function isArray(value) {
    return !Array.isArray ? getTag(value) === "[object Array]" : Array.isArray(value);
  }
  function baseToString(value) {
    if (typeof value == "string") return value;
    if (typeof value === "bigint") return value.toString();
    const result = value + "";
    return result == "0" && 1 / value == -Infinity ? "-0" : result;
  }
  function toString(value) {
    return value == null ? "" : baseToString(value);
  }
  function isString(value) {
    return typeof value === "string";
  }
  function isNumber(value) {
    return typeof value === "number";
  }
  function isBoolean(value) {
    return value === true || value === false || isObjectLike(value) && getTag(value) == "[object Boolean]";
  }
  function isObject(value) {
    return typeof value === "object";
  }
  function isObjectLike(value) {
    return isObject(value) && value !== null;
  }
  function isDefined(value) {
    return value !== void 0 && value !== null;
  }
  function isBlank(value) {
    return !value.trim().length;
  }
  function getTag(value) {
    return value == null ? value === void 0 ? "[object Undefined]" : "[object Null]" : Object.prototype.toString.call(value);
  }
  var INCORRECT_INDEX_TYPE = "Incorrect 'index' type";
  var INVALID_DOC_INDEX = "Invalid doc index: must be a non-negative integer within the bounds of the docs array";
  var LOGICAL_SEARCH_INVALID_QUERY_FOR_KEY = (key) => `Invalid value for key ${key}`;
  var PATTERN_LENGTH_TOO_LARGE = (max) => `Pattern length exceeds max of ${max}.`;
  var MISSING_KEY_PROPERTY = (name) => `Missing ${name} property in key`;
  var INVALID_KEY_WEIGHT_VALUE = (key) => `Property 'weight' in key '${key}' must be a positive integer`;
  var FUSE_MATCH_TOKEN_SEARCH_UNSUPPORTED = "Fuse.match does not support useTokenSearch: token search requires corpus-level statistics (df, fieldCount) that a one-off string comparison does not have. Use new Fuse(...).search(...) instead.";
  var hasOwn = Object.prototype.hasOwnProperty;
  var KeyStore = class {
    constructor(keys) {
      this._keys = [];
      this._keyMap = {};
      let totalWeight = 0;
      keys.forEach((key) => {
        const obj = createKey(key);
        this._keys.push(obj);
        this._keyMap[obj.id] = obj;
        totalWeight += obj.weight;
      });
      this._keys.forEach((key) => {
        key.weight /= totalWeight;
      });
    }
    get(keyId) {
      return this._keyMap[keyId];
    }
    keys() {
      return this._keys;
    }
    toJSON() {
      return JSON.stringify(this._keys);
    }
  };
  function createKey(key) {
    let path = null;
    let id = null;
    let src = null;
    let weight = 1;
    let getFn = null;
    if (isString(key) || isArray(key)) {
      src = key;
      path = createKeyPath(key);
      id = createKeyId(key);
    } else {
      if (!hasOwn.call(key, "name")) throw new Error(MISSING_KEY_PROPERTY("name"));
      const name = key.name;
      src = name;
      if (hasOwn.call(key, "weight") && key.weight !== void 0) {
        weight = key.weight;
        if (weight <= 0) throw new Error(INVALID_KEY_WEIGHT_VALUE(createKeyId(name)));
      }
      path = createKeyPath(name);
      id = createKeyId(name);
      getFn = key.getFn ?? null;
    }
    return {
      path,
      id,
      weight,
      src,
      getFn
    };
  }
  function createKeyPath(key) {
    return isArray(key) ? key : key.split(".");
  }
  function createKeyId(key) {
    return isArray(key) ? key.join(".") : key;
  }
  function get(obj, path) {
    const list = [];
    let arr = false;
    const deepGet = (obj2, path2, index, arrayIndex) => {
      if (!isDefined(obj2)) return;
      if (!path2[index]) list.push(arrayIndex !== void 0 ? {
        v: obj2,
        i: arrayIndex
      } : obj2);
      else {
        const value = obj2[path2[index]];
        if (!isDefined(value)) return;
        if (index === path2.length - 1 && (isString(value) || isNumber(value) || isBoolean(value) || typeof value === "bigint")) list.push(arrayIndex !== void 0 ? {
          v: toString(value),
          i: arrayIndex
        } : toString(value));
        else if (isArray(value)) {
          arr = true;
          for (let i = 0, len = value.length; i < len; i += 1) deepGet(value[i], path2, index + 1, i);
        } else if (path2.length) deepGet(value, path2, index + 1, arrayIndex);
      }
    };
    deepGet(obj, isString(path) ? path.split(".") : path, 0);
    return arr ? list : list[0];
  }
  var MatchOptions = {
    includeMatches: false,
    findAllMatches: false,
    minMatchCharLength: 1
  };
  var BasicOptions = {
    isCaseSensitive: false,
    ignoreDiacritics: false,
    includeScore: false,
    keys: [],
    shouldSort: true,
    sortFn: (a, b) => a.score === b.score ? a.idx < b.idx ? -1 : 1 : a.score < b.score ? -1 : 1
  };
  var FuzzyOptions = {
    location: 0,
    threshold: 0.6,
    distance: 100
  };
  var AdvancedOptions = {
    useExtendedSearch: false,
    useTokenSearch: false,
    tokenize: void 0,
    tokenMatch: "any",
    getFn: get,
    ignoreLocation: false,
    ignoreFieldNorm: false,
    fieldNormWeight: 1
  };
  var Config = Object.freeze({
    ...BasicOptions,
    ...MatchOptions,
    ...FuzzyOptions,
    ...AdvancedOptions
  });
  function isWordSeparator(code) {
    return code >= 9 && code <= 13 || code === 32 || code === 160;
  }
  function norm(weight = 1, mantissa = 3) {
    const cache = /* @__PURE__ */ new Map();
    const m = Math.pow(10, mantissa);
    return {
      get(value) {
        let numTokens = 0;
        let inWord = false;
        for (let i = 0; i < value.length; i++) if (!isWordSeparator(value.charCodeAt(i))) {
          if (!inWord) {
            numTokens++;
            inWord = true;
          }
        } else inWord = false;
        if (numTokens === 0) numTokens = 1;
        if (cache.has(numTokens)) return cache.get(numTokens);
        const n = Math.round(m / Math.pow(numTokens, 0.5 * weight)) / m;
        cache.set(numTokens, n);
        return n;
      },
      clear() {
        cache.clear();
      }
    };
  }
  var FuseIndex = class {
    constructor({ getFn = Config.getFn, fieldNormWeight = Config.fieldNormWeight } = {}) {
      this.norm = norm(fieldNormWeight, 3);
      this.getFn = getFn;
      this.isCreated = false;
      this.docs = [];
      this.keys = [];
      this._keysMap = {};
      this.setIndexRecords();
    }
    setSources(docs = []) {
      this.docs = docs;
    }
    setIndexRecords(records = []) {
      this.records = records;
    }
    setKeys(keys = []) {
      this.keys = keys;
      this._keysMap = {};
      keys.forEach((key, idx) => {
        this._keysMap[key.id] = idx;
      });
    }
    create() {
      if (this.isCreated || !this.docs.length) return;
      this.isCreated = true;
      const len = this.docs.length;
      this.records = new Array(len);
      let recordCount = 0;
      if (isString(this.docs[0])) for (let i = 0; i < len; i++) {
        const record = this._createStringRecord(this.docs[i], i);
        if (record) this.records[recordCount++] = record;
      }
      else for (let i = 0; i < len; i++) this.records[recordCount++] = this._createObjectRecord(this.docs[i], i);
      this.records.length = recordCount;
      this.norm.clear();
    }
    add(doc, docIndex) {
      if (!Number.isInteger(docIndex) || docIndex < 0) throw new Error(INVALID_DOC_INDEX);
      if (isString(doc)) {
        const record2 = this._createStringRecord(doc, docIndex);
        if (record2) this.records.push(record2);
        return record2;
      }
      const record = this._createObjectRecord(doc, docIndex);
      this.records.push(record);
      return record;
    }
    removeAt(idx) {
      if (!Number.isInteger(idx) || idx < 0) throw new Error(INVALID_DOC_INDEX);
      for (let i = 0, len = this.records.length; i < len; i += 1) if (this.records[i].i === idx) {
        this.records.splice(i, 1);
        break;
      }
      for (let i = 0, len = this.records.length; i < len; i += 1) if (this.records[i].i > idx) this.records[i].i -= 1;
    }
    removeAll(indices) {
      const toRemove = /* @__PURE__ */ new Set();
      for (const v of indices) if (Number.isInteger(v) && v >= 0) toRemove.add(v);
      if (toRemove.size === 0) return;
      this.records = this.records.filter((r) => !toRemove.has(r.i));
      const sorted = Array.from(toRemove).sort((a, b) => a - b);
      for (const record of this.records) {
        let lo = 0;
        let hi2 = sorted.length;
        while (lo < hi2) {
          const mid = lo + hi2 >>> 1;
          if (sorted[mid] < record.i) lo = mid + 1;
          else hi2 = mid;
        }
        record.i -= lo;
      }
    }
    getValueForItemAtKeyId(item, keyId) {
      return item[this._keysMap[keyId]];
    }
    size() {
      return this.records.length;
    }
    _createStringRecord(doc, docIndex) {
      if (!isDefined(doc) || isBlank(doc)) return null;
      return {
        v: doc,
        i: docIndex,
        n: this.norm.get(doc)
      };
    }
    _createObjectRecord(doc, docIndex) {
      const record = {
        i: docIndex,
        $: {}
      };
      for (let keyIndex = 0, keyLen = this.keys.length; keyIndex < keyLen; keyIndex++) {
        const key = this.keys[keyIndex];
        const value = key.getFn ? key.getFn(doc) : this.getFn(doc, key.path);
        if (!isDefined(value)) continue;
        if (isArray(value)) {
          const subRecords = [];
          for (let i = 0, len = value.length; i < len; i += 1) {
            const item = value[i];
            if (!isDefined(item)) continue;
            if (isString(item)) {
              if (!isBlank(item)) {
                const subRecord = {
                  v: item,
                  i,
                  n: this.norm.get(item)
                };
                subRecords.push(subRecord);
              }
            } else if (isDefined(item.v)) {
              const text = isString(item.v) ? item.v : toString(item.v);
              if (!isBlank(text)) {
                const subRecord = {
                  v: text,
                  i: item.i,
                  n: this.norm.get(text)
                };
                subRecords.push(subRecord);
              }
            }
          }
          record.$[keyIndex] = subRecords;
        } else if (isString(value) && !isBlank(value)) {
          const subRecord = {
            v: value,
            n: this.norm.get(value)
          };
          record.$[keyIndex] = subRecord;
        }
      }
      return record;
    }
    toJSON() {
      return {
        keys: this.keys.map(({ getFn, ...key }) => key),
        records: this.records
      };
    }
  };
  function createIndex(keys, docs, { getFn = Config.getFn, fieldNormWeight = Config.fieldNormWeight } = {}) {
    const myIndex = new FuseIndex({
      getFn,
      fieldNormWeight
    });
    myIndex.setKeys(keys.map(createKey));
    myIndex.setSources(docs);
    myIndex.create();
    return myIndex;
  }
  function parseIndex(data, { getFn = Config.getFn, fieldNormWeight = Config.fieldNormWeight } = {}) {
    const { keys, records } = data;
    const myIndex = new FuseIndex({
      getFn,
      fieldNormWeight
    });
    myIndex.setKeys(keys);
    myIndex.setIndexRecords(records);
    return myIndex;
  }
  function convertMaskToIndices(matchmask = [], minMatchCharLength = Config.minMatchCharLength) {
    const indices = [];
    let start = -1;
    let end = -1;
    let i = 0;
    for (let len = matchmask.length; i < len; i += 1) {
      const match = matchmask[i];
      if (match && start === -1) start = i;
      else if (!match && start !== -1) {
        end = i - 1;
        if (end - start + 1 >= minMatchCharLength) indices.push([start, end]);
        start = -1;
      }
    }
    if (matchmask[i - 1] && i - start >= minMatchCharLength) indices.push([start, i - 1]);
    return indices;
  }
  function search(text, pattern, patternAlphabet, { location = Config.location, distance = Config.distance, threshold = Config.threshold, findAllMatches = Config.findAllMatches, minMatchCharLength = Config.minMatchCharLength, includeMatches = Config.includeMatches, ignoreLocation = Config.ignoreLocation } = {}) {
    if (pattern.length > 32) throw new Error(PATTERN_LENGTH_TOO_LARGE(32));
    const patternLen = pattern.length;
    const textLen = text.length;
    const expectedLocation = Math.max(0, Math.min(location, textLen));
    let currentThreshold = threshold;
    let bestLocation = expectedLocation;
    const calcScore = (errors, currentLocation) => {
      const accuracy = errors / patternLen;
      if (ignoreLocation) return accuracy;
      const proximity = Math.abs(expectedLocation - currentLocation);
      if (!distance) return proximity ? 1 : accuracy;
      return accuracy + proximity / distance;
    };
    const computeMatches = minMatchCharLength > 1 || includeMatches;
    const matchMask = computeMatches ? Array(textLen) : [];
    let index;
    while ((index = text.indexOf(pattern, bestLocation)) > -1) {
      const score = calcScore(0, index);
      currentThreshold = Math.min(score, currentThreshold);
      bestLocation = index + patternLen;
      if (computeMatches) {
        let i = 0;
        while (i < patternLen) {
          matchMask[index + i] = 1;
          i += 1;
        }
      }
    }
    bestLocation = -1;
    let lastBitArr = [];
    let finalScore = 1;
    let bestErrors = 0;
    let binMax = patternLen + textLen;
    const mask = 1 << patternLen - 1;
    for (let i = 0; i < patternLen; i += 1) {
      let binMin = 0;
      let binMid = binMax;
      while (binMin < binMid) {
        if (calcScore(i, expectedLocation + binMid) <= currentThreshold) binMin = binMid;
        else binMax = binMid;
        binMid = Math.floor((binMax - binMin) / 2 + binMin);
      }
      binMax = binMid;
      let start = Math.max(1, expectedLocation - binMid + 1);
      const finish = findAllMatches ? textLen : Math.min(expectedLocation + binMid, textLen) + patternLen;
      const bitArr = Array(finish + 2);
      bitArr[finish + 1] = (1 << i) - 1;
      for (let j = finish; j >= start; j -= 1) {
        const currentLocation = j - 1;
        const charMatch = patternAlphabet[text[currentLocation]];
        bitArr[j] = (bitArr[j + 1] << 1 | 1) & charMatch;
        if (i) bitArr[j] |= (lastBitArr[j + 1] | lastBitArr[j]) << 1 | 1 | lastBitArr[j + 1];
        if (bitArr[j] & mask) {
          finalScore = calcScore(i, currentLocation);
          if (finalScore <= currentThreshold) {
            currentThreshold = finalScore;
            bestLocation = currentLocation;
            bestErrors = i;
            if (bestLocation <= expectedLocation) break;
            start = Math.max(1, 2 * expectedLocation - bestLocation);
          }
        }
      }
      if (calcScore(i + 1, expectedLocation) > currentThreshold) break;
      lastBitArr = bitArr;
    }
    if (computeMatches && bestLocation >= 0) {
      const matchEnd = Math.min(textLen - 1, bestLocation + patternLen - 1 + bestErrors);
      for (let k = bestLocation; k <= matchEnd; k += 1) if (patternAlphabet[text[k]]) matchMask[k] = 1;
    }
    const result = {
      isMatch: bestLocation >= 0,
      score: Math.max(1e-3, finalScore)
    };
    if (computeMatches) {
      const indices = convertMaskToIndices(matchMask, minMatchCharLength);
      if (!indices.length) result.isMatch = false;
      else if (includeMatches) result.indices = indices;
    }
    return result;
  }
  function createPatternAlphabet(pattern) {
    const mask = {};
    for (let i = 0, len = pattern.length; i < len; i += 1) {
      const char = pattern.charAt(i);
      mask[char] = (mask[char] || 0) | 1 << len - i - 1;
    }
    return mask;
  }
  function mergeIndices(indices) {
    if (indices.length <= 1) return indices;
    indices.sort((a, b) => a[0] - b[0] || a[1] - b[1]);
    const merged = [indices[0]];
    for (let i = 1, len = indices.length; i < len; i += 1) {
      const last = merged[merged.length - 1];
      const curr = indices[i];
      if (curr[0] <= last[1] + 1) last[1] = Math.max(last[1], curr[1]);
      else merged.push(curr);
    }
    return merged;
  }
  var NON_DECOMPOSABLE_MAP = {
    "\u0142": "l",
    "\u0141": "L",
    "\u0111": "d",
    "\u0110": "D",
    "\xF8": "o",
    "\xD8": "O",
    "\u0127": "h",
    "\u0126": "H",
    "\u0167": "t",
    "\u0166": "T",
    "\u0131": "i",
    "\xDF": "ss"
  };
  var NON_DECOMPOSABLE_RE = new RegExp("[" + Object.keys(NON_DECOMPOSABLE_MAP).join("") + "]", "g");
  var stripDiacritics = typeof String.prototype.normalize === "function" ? (str) => str.normalize("NFD").replace(/[\u0300-\u036F\u0483-\u0489\u0591-\u05BD\u05BF\u05C1\u05C2\u05C4\u05C5\u05C7\u0610-\u061A\u064B-\u065F\u0670\u06D6-\u06DC\u06DF-\u06E4\u06E7\u06E8\u06EA-\u06ED\u0711\u0730-\u074A\u07A6-\u07B0\u07EB-\u07F3\u07FD\u0816-\u0819\u081B-\u0823\u0825-\u0827\u0829-\u082D\u0859-\u085B\u08D3-\u08E1\u08E3-\u0903\u093A-\u093C\u093E-\u094F\u0951-\u0957\u0962\u0963\u0981-\u0983\u09BC\u09BE-\u09C4\u09C7\u09C8\u09CB-\u09CD\u09D7\u09E2\u09E3\u09FE\u0A01-\u0A03\u0A3C\u0A3E-\u0A42\u0A47\u0A48\u0A4B-\u0A4D\u0A51\u0A70\u0A71\u0A75\u0A81-\u0A83\u0ABC\u0ABE-\u0AC5\u0AC7-\u0AC9\u0ACB-\u0ACD\u0AE2\u0AE3\u0AFA-\u0AFF\u0B01-\u0B03\u0B3C\u0B3E-\u0B44\u0B47\u0B48\u0B4B-\u0B4D\u0B56\u0B57\u0B62\u0B63\u0B82\u0BBE-\u0BC2\u0BC6-\u0BC8\u0BCA-\u0BCD\u0BD7\u0C00-\u0C04\u0C3E-\u0C44\u0C46-\u0C48\u0C4A-\u0C4D\u0C55\u0C56\u0C62\u0C63\u0C81-\u0C83\u0CBC\u0CBE-\u0CC4\u0CC6-\u0CC8\u0CCA-\u0CCD\u0CD5\u0CD6\u0CE2\u0CE3\u0D00-\u0D03\u0D3B\u0D3C\u0D3E-\u0D44\u0D46-\u0D48\u0D4A-\u0D4D\u0D57\u0D62\u0D63\u0D82\u0D83\u0DCA\u0DCF-\u0DD4\u0DD6\u0DD8-\u0DDF\u0DF2\u0DF3\u0E31\u0E34-\u0E3A\u0E47-\u0E4E\u0EB1\u0EB4-\u0EB9\u0EBB\u0EBC\u0EC8-\u0ECD\u0F18\u0F19\u0F35\u0F37\u0F39\u0F3E\u0F3F\u0F71-\u0F84\u0F86\u0F87\u0F8D-\u0F97\u0F99-\u0FBC\u0FC6\u102B-\u103E\u1056-\u1059\u105E-\u1060\u1062-\u1064\u1067-\u106D\u1071-\u1074\u1082-\u108D\u108F\u109A-\u109D\u135D-\u135F\u1712-\u1714\u1732-\u1734\u1752\u1753\u1772\u1773\u17B4-\u17D3\u17DD\u180B-\u180D\u1885\u1886\u18A9\u1920-\u192B\u1930-\u193B\u1A17-\u1A1B\u1A55-\u1A5E\u1A60-\u1A7C\u1A7F\u1AB0-\u1ABE\u1B00-\u1B04\u1B34-\u1B44\u1B6B-\u1B73\u1B80-\u1B82\u1BA1-\u1BAD\u1BE6-\u1BF3\u1C24-\u1C37\u1CD0-\u1CD2\u1CD4-\u1CE8\u1CED\u1CF2-\u1CF4\u1CF7-\u1CF9\u1DC0-\u1DF9\u1DFB-\u1DFF\u20D0-\u20F0\u2CEF-\u2CF1\u2D7F\u2DE0-\u2DFF\u302A-\u302F\u3099\u309A\uA66F-\uA672\uA674-\uA67D\uA69E\uA69F\uA6F0\uA6F1\uA802\uA806\uA80B\uA823-\uA827\uA880\uA881\uA8B4-\uA8C5\uA8E0-\uA8F1\uA8FF\uA926-\uA92D\uA947-\uA953\uA980-\uA983\uA9B3-\uA9C0\uA9E5\uAA29-\uAA36\uAA43\uAA4C\uAA4D\uAA7B-\uAA7D\uAAB0\uAAB2-\uAAB4\uAAB7\uAAB8\uAABE\uAABF\uAAC1\uAAEB-\uAAEF\uAAF5\uAAF6\uABE3-\uABEA\uABEC\uABED\uFB1E\uFE00-\uFE0F\uFE20-\uFE2F]/g, "").replace(NON_DECOMPOSABLE_RE, (ch) => NON_DECOMPOSABLE_MAP[ch]) : (str) => str;
  var BitapSearch = class {
    constructor(pattern, { location = Config.location, threshold = Config.threshold, distance = Config.distance, includeMatches = Config.includeMatches, findAllMatches = Config.findAllMatches, minMatchCharLength = Config.minMatchCharLength, isCaseSensitive = Config.isCaseSensitive, ignoreDiacritics = Config.ignoreDiacritics, ignoreLocation = Config.ignoreLocation } = {}) {
      this.options = {
        location,
        threshold,
        distance,
        includeMatches,
        findAllMatches,
        minMatchCharLength,
        isCaseSensitive,
        ignoreDiacritics,
        ignoreLocation
      };
      pattern = isCaseSensitive ? pattern : pattern.toLowerCase();
      pattern = ignoreDiacritics ? stripDiacritics(pattern) : pattern;
      this.pattern = pattern;
      this.chunks = [];
      if (!this.pattern.length) return;
      const addChunk = (pattern2, startIndex) => {
        this.chunks.push({
          pattern: pattern2,
          alphabet: createPatternAlphabet(pattern2),
          startIndex
        });
      };
      const len = this.pattern.length;
      if (len > 32) {
        let i = 0;
        const remainder = len % 32;
        const end = len - remainder;
        while (i < end) {
          addChunk(this.pattern.substr(i, 32), i);
          i += 32;
        }
        if (remainder) {
          const startIndex = len - 32;
          addChunk(this.pattern.substr(startIndex), startIndex);
        }
      } else addChunk(this.pattern, 0);
    }
    searchIn(text) {
      const { isCaseSensitive, ignoreDiacritics, includeMatches } = this.options;
      text = isCaseSensitive ? text : text.toLowerCase();
      text = ignoreDiacritics ? stripDiacritics(text) : text;
      if (this.pattern === text) {
        if (text.length < this.options.minMatchCharLength) return {
          isMatch: false,
          score: 1
        };
        const result2 = {
          isMatch: true,
          score: 0
        };
        if (includeMatches) result2.indices = [[0, text.length - 1]];
        return result2;
      }
      const { location, distance, threshold, findAllMatches, minMatchCharLength, ignoreLocation } = this.options;
      const allIndices = [];
      let totalScore = 0;
      let hasMatches = false;
      this.chunks.forEach(({ pattern, alphabet, startIndex }) => {
        const { isMatch, score, indices } = search(text, pattern, alphabet, {
          location: location + startIndex,
          distance,
          threshold,
          findAllMatches,
          minMatchCharLength,
          includeMatches,
          ignoreLocation
        });
        if (isMatch) hasMatches = true;
        totalScore += score;
        if (isMatch && indices) allIndices.push(...indices);
      });
      const result = {
        isMatch: hasMatches,
        score: hasMatches ? totalScore / this.chunks.length : 1
      };
      if (hasMatches && includeMatches) result.indices = mergeIndices(allIndices);
      return result;
    }
  };
  var MULTI_MATCH_TYPES = /* @__PURE__ */ new Set(["fuzzy", "include"]);
  function isInverse(type) {
    return type.startsWith("inverse");
  }
  var matchers = [
    {
      type: "exact",
      multiRegex: /^="(.*)"$/,
      singleRegex: /^=(.*)$/,
      create: (pattern) => ({
        type: "exact",
        search(text) {
          const isMatch = text === pattern;
          return {
            isMatch,
            score: isMatch ? 0 : 1,
            indices: [0, pattern.length - 1]
          };
        }
      })
    },
    {
      type: "include",
      multiRegex: /^'"(.*)"$/,
      singleRegex: /^'(.*)$/,
      create: (pattern) => ({
        type: "include",
        search(text) {
          let location = 0;
          let index;
          const indices = [];
          const patternLen = pattern.length;
          while ((index = text.indexOf(pattern, location)) > -1) {
            location = index + patternLen;
            indices.push([index, location - 1]);
          }
          const isMatch = !!indices.length;
          return {
            isMatch,
            score: isMatch ? 0 : 1,
            indices
          };
        }
      })
    },
    {
      type: "prefix-exact",
      multiRegex: /^\^"(.*)"$/,
      singleRegex: /^\^(.*)$/,
      create: (pattern) => ({
        type: "prefix-exact",
        search(text) {
          const isMatch = text.startsWith(pattern);
          return {
            isMatch,
            score: isMatch ? 0 : 1,
            indices: [0, pattern.length - 1]
          };
        }
      })
    },
    {
      type: "inverse-prefix-exact",
      multiRegex: /^!\^"(.*)"$/,
      singleRegex: /^!\^(.*)$/,
      create: (pattern) => ({
        type: "inverse-prefix-exact",
        search(text) {
          const isMatch = !text.startsWith(pattern);
          return {
            isMatch,
            score: isMatch ? 0 : 1,
            indices: [0, text.length - 1]
          };
        }
      })
    },
    {
      type: "inverse-suffix-exact",
      multiRegex: /^!"(.*)"\$$/,
      singleRegex: /^!(.*)\$$/,
      create: (pattern) => ({
        type: "inverse-suffix-exact",
        search(text) {
          const isMatch = !text.endsWith(pattern);
          return {
            isMatch,
            score: isMatch ? 0 : 1,
            indices: [0, text.length - 1]
          };
        }
      })
    },
    {
      type: "suffix-exact",
      multiRegex: /^"(.*)"\$$/,
      singleRegex: /^(.*)\$$/,
      create: (pattern) => ({
        type: "suffix-exact",
        search(text) {
          const isMatch = text.endsWith(pattern);
          return {
            isMatch,
            score: isMatch ? 0 : 1,
            indices: [text.length - pattern.length, text.length - 1]
          };
        }
      })
    },
    {
      type: "inverse-exact",
      multiRegex: /^!"(.*)"$/,
      singleRegex: /^!(.*)$/,
      create: (pattern) => ({
        type: "inverse-exact",
        search(text) {
          const isMatch = text.indexOf(pattern) === -1;
          return {
            isMatch,
            score: isMatch ? 0 : 1,
            indices: [0, text.length - 1]
          };
        }
      })
    },
    {
      type: "fuzzy",
      multiRegex: /^"(.*)"$/,
      singleRegex: /^(.*)$/,
      create: (pattern, options = {}) => {
        const bitap = new BitapSearch(pattern, {
          location: options.location ?? Config.location,
          threshold: options.threshold ?? Config.threshold,
          distance: options.distance ?? Config.distance,
          includeMatches: options.includeMatches ?? Config.includeMatches,
          findAllMatches: options.findAllMatches ?? Config.findAllMatches,
          minMatchCharLength: options.minMatchCharLength ?? Config.minMatchCharLength,
          isCaseSensitive: options.isCaseSensitive ?? Config.isCaseSensitive,
          ignoreDiacritics: options.ignoreDiacritics ?? Config.ignoreDiacritics,
          ignoreLocation: options.ignoreLocation ?? Config.ignoreLocation
        });
        return {
          type: "fuzzy",
          search(text) {
            return bitap.searchIn(text);
          }
        };
      }
    }
  ];
  var matchersLen = matchers.length;
  var ESCAPED_PIPE = "\0";
  var OR_TOKEN = "|";
  function tokenize(pattern) {
    const tokens = [];
    const len = pattern.length;
    let i = 0;
    while (i < len) {
      while (i < len && pattern[i] === " ") i++;
      if (i >= len) break;
      let j = i;
      while (j < len && pattern[j] !== " " && pattern[j] !== '"') j++;
      if (j < len && pattern[j] === '"') {
        j++;
        while (j < len) {
          if (pattern[j] === '"') {
            const next = j + 1;
            if (next >= len || pattern[next] === " ") {
              j++;
              break;
            }
            if (pattern[next] === "$" && (next + 1 >= len || pattern[next + 1] === " ")) {
              j += 2;
              break;
            }
          }
          j++;
        }
        tokens.push(pattern.substring(i, j));
        i = j;
      } else {
        while (j < len && pattern[j] !== " ") j++;
        tokens.push(pattern.substring(i, j));
        i = j;
      }
    }
    return tokens;
  }
  function getMatch(pattern, exp) {
    const matches = pattern.match(exp);
    return matches ? matches[1] : null;
  }
  function parseQuery(pattern, options = {}) {
    return pattern.replace(/\\\|/g, ESCAPED_PIPE).split(OR_TOKEN).map((item) => {
      const query = tokenize(item.replace(/\u0000/g, "|").trim()).filter((item2) => item2 && !!item2.trim());
      const results = [];
      for (let i = 0, len = query.length; i < len; i += 1) {
        const queryItem = query[i];
        let found = false;
        let idx = -1;
        while (!found && ++idx < matchersLen) {
          const def = matchers[idx];
          const token = getMatch(queryItem, def.multiRegex);
          if (token) {
            results.push(def.create(token, options));
            found = true;
          }
        }
        if (found) continue;
        idx = -1;
        while (++idx < matchersLen) {
          const def = matchers[idx];
          const token = getMatch(queryItem, def.singleRegex);
          if (token) {
            results.push(def.create(token, options));
            break;
          }
        }
      }
      return results;
    });
  }
  var ExtendedSearch = class {
    constructor(pattern, { isCaseSensitive = Config.isCaseSensitive, ignoreDiacritics = Config.ignoreDiacritics, includeMatches = Config.includeMatches, minMatchCharLength = Config.minMatchCharLength, ignoreLocation = Config.ignoreLocation, findAllMatches = Config.findAllMatches, location = Config.location, threshold = Config.threshold, distance = Config.distance } = {}) {
      this.query = null;
      this.options = {
        isCaseSensitive,
        ignoreDiacritics,
        includeMatches,
        minMatchCharLength,
        findAllMatches,
        ignoreLocation,
        location,
        threshold,
        distance
      };
      pattern = isCaseSensitive ? pattern : pattern.toLowerCase();
      pattern = ignoreDiacritics ? stripDiacritics(pattern) : pattern;
      this.pattern = pattern;
      this.query = parseQuery(this.pattern, this.options);
    }
    static condition(_, options) {
      return options.useExtendedSearch;
    }
    searchIn(text) {
      const query = this.query;
      if (!query) return {
        isMatch: false,
        score: 1
      };
      const { includeMatches, isCaseSensitive, ignoreDiacritics } = this.options;
      text = isCaseSensitive ? text : text.toLowerCase();
      text = ignoreDiacritics ? stripDiacritics(text) : text;
      let numMatches = 0;
      const allIndices = [];
      let totalScore = 0;
      let hasInverse = false;
      for (let i = 0, qLen = query.length; i < qLen; i += 1) {
        const searchers = query[i];
        allIndices.length = 0;
        numMatches = 0;
        hasInverse = false;
        for (let j = 0, pLen = searchers.length; j < pLen; j += 1) {
          const matcher = searchers[j];
          const { isMatch, indices, score } = matcher.search(text);
          if (isMatch) {
            numMatches += 1;
            totalScore += score;
            if (isInverse(matcher.type)) hasInverse = true;
            if (includeMatches) if (MULTI_MATCH_TYPES.has(matcher.type)) allIndices.push(...indices);
            else allIndices.push(indices);
          } else {
            totalScore = 0;
            numMatches = 0;
            allIndices.length = 0;
            hasInverse = false;
            break;
          }
        }
        if (numMatches) {
          const result = {
            isMatch: true,
            score: totalScore / numMatches
          };
          if (hasInverse) result.hasInverse = true;
          if (includeMatches) result.indices = mergeIndices(allIndices);
          return result;
        }
      }
      return {
        isMatch: false,
        score: 1
      };
    }
  };
  var registeredSearchers = [];
  function register(...args) {
    registeredSearchers.push(...args);
  }
  function createSearcher(pattern, options) {
    for (let i = 0, len = registeredSearchers.length; i < len; i += 1) {
      const searcherClass = registeredSearchers[i];
      if (searcherClass.condition(pattern, options)) return new searcherClass(pattern, options);
    }
    return new BitapSearch(pattern, options);
  }
  var LogicalOperator = {
    AND: "$and",
    OR: "$or"
  };
  var KeyType = {
    PATH: "$path",
    PATTERN: "$val"
  };
  var isExpression = (query) => !!(query[LogicalOperator.AND] || query[LogicalOperator.OR]);
  var isPath = (query) => !!query[KeyType.PATH];
  var isLeaf = (query) => !isArray(query) && isObject(query) && !isExpression(query);
  var convertToExplicit = (query) => ({ [LogicalOperator.AND]: Object.keys(query).map((key) => ({ [key]: query[key] })) });
  function parse(query, options, { auto = true } = {}) {
    const next = (query2) => {
      if (isString(query2)) {
        const obj = {
          keyId: null,
          pattern: query2
        };
        if (auto) obj.searcher = createSearcher(query2, options);
        return obj;
      }
      const keys = Object.keys(query2);
      const isQueryPath = isPath(query2);
      if (!isQueryPath && keys.length > 1 && !isExpression(query2)) return next(convertToExplicit(query2));
      if (isLeaf(query2)) {
        const key = isQueryPath ? query2[KeyType.PATH] : keys[0];
        const pattern = isQueryPath ? query2[KeyType.PATTERN] : query2[key];
        if (!isString(pattern)) throw new Error(LOGICAL_SEARCH_INVALID_QUERY_FOR_KEY(key));
        const obj = {
          keyId: createKeyId(key),
          pattern
        };
        if (auto) obj.searcher = createSearcher(pattern, options);
        return obj;
      }
      const node = {
        children: [],
        operator: keys[0]
      };
      keys.forEach((key) => {
        const value = query2[key];
        if (isArray(value)) value.forEach((item) => {
          node.children.push(next(item));
        });
      });
      return node;
    };
    if (!isExpression(query)) query = convertToExplicit(query);
    return next(query);
  }
  function computeScoreSingle(matches, { ignoreFieldNorm = Config.ignoreFieldNorm }) {
    let totalScore = 1;
    matches.forEach(({ key, norm: norm2, score }) => {
      const weight = key ? key.weight : null;
      totalScore *= Math.pow(score === 0 && weight ? Number.EPSILON : score, (weight || 1) * (ignoreFieldNorm ? 1 : norm2));
    });
    return totalScore;
  }
  function computeScore(results, { ignoreFieldNorm = Config.ignoreFieldNorm }) {
    results.forEach((result) => {
      result.score = computeScoreSingle(result.matches, { ignoreFieldNorm });
    });
  }
  var MaxHeap = class {
    constructor(limit, comparator) {
      this.limit = limit;
      this.heap = [];
      this.comparator = comparator;
    }
    get size() {
      return this.heap.length;
    }
    insert(item) {
      if (this.size < this.limit) {
        this.heap.push(item);
        this._bubbleUp(this.size - 1);
      } else if (this.comparator(item, this.heap[0]) < 0) {
        this.heap[0] = item;
        this._sinkDown(0);
      }
    }
    extractSorted() {
      return this.heap.sort(this.comparator);
    }
    _bubbleUp(i) {
      const heap = this.heap;
      while (i > 0) {
        const parent = i - 1 >> 1;
        if (this.comparator(heap[i], heap[parent]) <= 0) break;
        const tmp = heap[i];
        heap[i] = heap[parent];
        heap[parent] = tmp;
        i = parent;
      }
    }
    _sinkDown(i) {
      const heap = this.heap;
      const len = heap.length;
      let largest = i;
      do {
        i = largest;
        const left = 2 * i + 1;
        const right = 2 * i + 2;
        if (left < len && this.comparator(heap[left], heap[largest]) > 0) largest = left;
        if (right < len && this.comparator(heap[right], heap[largest]) > 0) largest = right;
        if (largest !== i) {
          const tmp = heap[i];
          heap[i] = heap[largest];
          heap[largest] = tmp;
        }
      } while (largest !== i);
    }
  };
  function formatMatches(result) {
    const matches = [];
    result.matches.forEach((match) => {
      if (!isDefined(match.indices) || !match.indices.length) return;
      const obj = {
        indices: match.indices,
        value: match.value
      };
      if (match.key) obj.key = match.key.id;
      if (match.idx > -1) obj.refIndex = match.idx;
      matches.push(obj);
    });
    return matches;
  }
  function format(results, docs, { includeMatches = Config.includeMatches, includeScore = Config.includeScore } = {}) {
    return results.map((result) => {
      const { idx } = result;
      const data = {
        item: docs[idx],
        refIndex: idx
      };
      if (includeMatches) data.matches = formatMatches(result);
      if (includeScore) data.score = result.score;
      return data;
    });
  }
  var DEFAULT_TOKEN = /[\p{L}\p{M}\p{N}_]+/gu;
  var warned = /* @__PURE__ */ new WeakSet();
  function warnNonGlobal(regex) {
    if (!warned.has(regex)) {
      warned.add(regex);
      console.warn(`[Fuse] tokenize regex ${regex} lacks the global flag; only the first match per text will be returned. Add the 'g' flag.`);
    }
  }
  function resolveTokenize(tokenize2) {
    if (typeof tokenize2 === "function") {
      let validated = false;
      return (text) => {
        const result = tokenize2(text);
        if (!validated) {
          validated = true;
          if (!Array.isArray(result) || result.some((t) => typeof t !== "string")) throw new Error(`[Fuse] tokenize function must return string[]; received ${Array.isArray(result) ? "array containing non-strings" : typeof result}.`);
        }
        return result;
      };
    }
    if (tokenize2 instanceof RegExp) {
      if (!tokenize2.global) warnNonGlobal(tokenize2);
      return (text) => text.match(tokenize2) || [];
    }
    return (text) => text.match(DEFAULT_TOKEN) || [];
  }
  function createAnalyzer({ isCaseSensitive = false, ignoreDiacritics = false, tokenize: tokenize2 } = {}) {
    const tokenizeFn = resolveTokenize(tokenize2);
    return { tokenize(text) {
      if (!isCaseSensitive) text = text.toLowerCase();
      if (ignoreDiacritics) text = stripDiacritics(text);
      return tokenizeFn(text);
    } };
  }
  var TokenSearch = class {
    static condition(_, options) {
      return options.useTokenSearch;
    }
    constructor(pattern, options) {
      this.options = options;
      this.analyzer = createAnalyzer({
        isCaseSensitive: options.isCaseSensitive,
        ignoreDiacritics: options.ignoreDiacritics,
        tokenize: options.tokenize
      });
      const queryTerms = this.analyzer.tokenize(pattern);
      const { df, fieldCount } = options._invertedIndex;
      this.termSearchers = [];
      this.idfWeights = [];
      for (const term2 of queryTerms) {
        this.termSearchers.push(new BitapSearch(term2, {
          location: options.location,
          threshold: options.threshold,
          distance: options.distance,
          includeMatches: options.includeMatches,
          findAllMatches: options.findAllMatches,
          minMatchCharLength: options.minMatchCharLength,
          isCaseSensitive: options.isCaseSensitive,
          ignoreDiacritics: options.ignoreDiacritics,
          ignoreLocation: true
        }));
        const docFreq = df.get(term2) || 0;
        const idf = Math.log(1 + (fieldCount - docFreq + 0.5) / (docFreq + 0.5));
        this.idfWeights.push(idf);
      }
      this.combineAll = options.tokenMatch === "all";
      this.numTerms = this.termSearchers.length;
      this.useMask = this.numTerms <= 31;
    }
    searchIn(text) {
      if (!this.termSearchers.length) return {
        isMatch: false,
        score: 1
      };
      const allIndices = [];
      let weightedScore = 0;
      let maxPossibleScore = 0;
      let matchedCount = 0;
      let matchedMask = 0;
      const matchedTerms = this.combineAll && !this.useMask ? /* @__PURE__ */ new Set() : null;
      for (let i = 0; i < this.termSearchers.length; i++) {
        const result = this.termSearchers[i].searchIn(text);
        const idf = this.idfWeights[i];
        maxPossibleScore += idf;
        if (result.isMatch) {
          matchedCount++;
          weightedScore += idf * (1 - result.score);
          if (result.indices) allIndices.push(...result.indices);
          if (this.combineAll) if (this.useMask) matchedMask |= 1 << i;
          else matchedTerms.add(i);
        }
      }
      if (matchedCount === 0) return {
        isMatch: false,
        score: 1
      };
      const normalized = maxPossibleScore > 0 ? 1 - weightedScore / maxPossibleScore : 0;
      const searchResult = {
        isMatch: true,
        score: Math.max(1e-3, normalized)
      };
      if (this.options.includeMatches && allIndices.length) searchResult.indices = mergeIndices(allIndices);
      if (this.combineAll) {
        if (this.useMask) searchResult.matchedMask = matchedMask;
        else searchResult.matchedTerms = matchedTerms;
        searchResult.termCount = this.numTerms;
      }
      return searchResult;
    }
  };
  function addField(index, text, docIdx, analyzer) {
    const tokens = analyzer.tokenize(text);
    if (!tokens.length) return;
    index.fieldCount++;
    index.docFieldCount.set(docIdx, (index.docFieldCount.get(docIdx) || 0) + 1);
    const distinctTerms = new Set(tokens);
    let perDocTerms = index.docTermFieldHits.get(docIdx);
    if (!perDocTerms) {
      perDocTerms = /* @__PURE__ */ new Map();
      index.docTermFieldHits.set(docIdx, perDocTerms);
    }
    for (const term2 of distinctTerms) {
      perDocTerms.set(term2, (perDocTerms.get(term2) || 0) + 1);
      index.df.set(term2, (index.df.get(term2) || 0) + 1);
    }
  }
  function ingestRecord(index, record, keyCount, analyzer) {
    const { i: docIdx, v, $: fields } = record;
    if (v !== void 0) {
      addField(index, v, docIdx, analyzer);
      return;
    }
    if (!fields) return;
    for (let keyIdx = 0; keyIdx < keyCount; keyIdx++) {
      const value = fields[keyIdx];
      if (!value) continue;
      if (Array.isArray(value)) for (const sub of value) addField(index, sub.v, docIdx, analyzer);
      else addField(index, value.v, docIdx, analyzer);
    }
  }
  function buildInvertedIndex(records, keyCount, analyzer) {
    const index = {
      fieldCount: 0,
      df: /* @__PURE__ */ new Map(),
      docFieldCount: /* @__PURE__ */ new Map(),
      docTermFieldHits: /* @__PURE__ */ new Map()
    };
    for (const record of records) ingestRecord(index, record, keyCount, analyzer);
    return index;
  }
  function addToInvertedIndex(index, record, keyCount, analyzer) {
    ingestRecord(index, record, keyCount, analyzer);
  }
  function removeFromInvertedIndex(index, docIdx) {
    const fieldCount = index.docFieldCount.get(docIdx);
    if (fieldCount === void 0) return;
    index.fieldCount -= fieldCount;
    index.docFieldCount.delete(docIdx);
    const perDocTerms = index.docTermFieldHits.get(docIdx);
    if (!perDocTerms) return;
    for (const [term2, hits] of perDocTerms) {
      const next = (index.df.get(term2) || 0) - hits;
      if (next <= 0) index.df.delete(term2);
      else index.df.set(term2, next);
    }
    index.docTermFieldHits.delete(docIdx);
  }
  function removeAndShiftInvertedIndex(index, removedIndices) {
    if (removedIndices.length === 0) return;
    const sorted = Array.from(new Set(removedIndices)).sort((a, b) => a - b);
    for (const idx of sorted) removeFromInvertedIndex(index, idx);
    const shift = (oldIdx) => {
      let lo = 0;
      let hi2 = sorted.length;
      while (lo < hi2) {
        const mid = lo + hi2 >>> 1;
        if (sorted[mid] < oldIdx) lo = mid + 1;
        else hi2 = mid;
      }
      return oldIdx - lo;
    };
    const firstRemoved = sorted[0];
    const shiftedDocFieldCount = /* @__PURE__ */ new Map();
    for (const [oldKey, count] of index.docFieldCount) shiftedDocFieldCount.set(oldKey > firstRemoved ? shift(oldKey) : oldKey, count);
    index.docFieldCount = shiftedDocFieldCount;
    const shiftedDocTermFieldHits = /* @__PURE__ */ new Map();
    for (const [oldKey, terms] of index.docTermFieldHits) shiftedDocTermFieldHits.set(oldKey > firstRemoved ? shift(oldKey) : oldKey, terms);
    index.docTermFieldHits = shiftedDocTermFieldHits;
  }
  var Fuse = class {
    constructor(docs, options, index) {
      this.options = {
        ...Config,
        ...options
      };
      if (this.options.useExtendedSearch && false) ;
      if (this.options.useTokenSearch && false) ;
      this._keyStore = new KeyStore(this.options.keys);
      this._docs = docs;
      this._myIndex = null;
      this._invertedIndex = null;
      this.setCollection(docs, index);
      this._lastQuery = null;
      this._lastSearcher = null;
    }
    _getSearcher(query) {
      if (this._lastQuery === query) return this._lastSearcher;
      const searcher = createSearcher(query, this._invertedIndex ? {
        ...this.options,
        _invertedIndex: this._invertedIndex
      } : this.options);
      this._lastQuery = query;
      this._lastSearcher = searcher;
      return searcher;
    }
    setCollection(docs, index) {
      this._docs = docs;
      if (index && !(index instanceof FuseIndex)) throw new Error(INCORRECT_INDEX_TYPE);
      this._myIndex = index || createIndex(this.options.keys, this._docs, {
        getFn: this.options.getFn,
        fieldNormWeight: this.options.fieldNormWeight
      });
      if (this.options.useTokenSearch) {
        const analyzer = createAnalyzer({
          isCaseSensitive: this.options.isCaseSensitive,
          ignoreDiacritics: this.options.ignoreDiacritics,
          tokenize: this.options.tokenize
        });
        this._invertedIndex = buildInvertedIndex(this._myIndex.records, this._myIndex.keys.length, analyzer);
      }
      this._invalidateSearcherCache();
    }
    add(doc) {
      if (!isDefined(doc)) return;
      this._docs.push(doc);
      const record = this._myIndex.add(doc, this._docs.length - 1);
      if (this._invertedIndex && record) {
        const analyzer = createAnalyzer({
          isCaseSensitive: this.options.isCaseSensitive,
          ignoreDiacritics: this.options.ignoreDiacritics,
          tokenize: this.options.tokenize
        });
        addToInvertedIndex(this._invertedIndex, record, this._myIndex.keys.length, analyzer);
      }
      this._invalidateSearcherCache();
    }
    remove(predicate = () => false) {
      const results = [];
      const indicesToRemove = [];
      for (let i = 0, len = this._docs.length; i < len; i += 1) if (predicate(this._docs[i], i)) {
        results.push(this._docs[i]);
        indicesToRemove.push(i);
      }
      if (indicesToRemove.length) {
        if (this._invertedIndex) removeAndShiftInvertedIndex(this._invertedIndex, indicesToRemove);
        const toRemove = new Set(indicesToRemove);
        this._docs = this._docs.filter((_, i) => !toRemove.has(i));
        this._myIndex.removeAll(indicesToRemove);
        this._invalidateSearcherCache();
      }
      return results;
    }
    removeAt(idx) {
      if (!Number.isInteger(idx) || idx < 0 || idx >= this._docs.length) throw new Error(INVALID_DOC_INDEX);
      if (this._invertedIndex) removeAndShiftInvertedIndex(this._invertedIndex, [idx]);
      const doc = this._docs.splice(idx, 1)[0];
      this._myIndex.removeAt(idx);
      this._invalidateSearcherCache();
      return doc;
    }
    _invalidateSearcherCache() {
      this._lastQuery = null;
      this._lastSearcher = null;
    }
    getIndex() {
      return this._myIndex;
    }
    _normalizedKeys() {
      return this._myIndex.keys.map((key) => this._keyStore.get(key.id) || key);
    }
    search(query, options) {
      const { limit = -1 } = options || {};
      const { includeMatches, includeScore, shouldSort, sortFn, ignoreFieldNorm } = this.options;
      if (isString(query) && !query.trim()) {
        let docs = this._docs.map((item, idx) => ({
          item,
          refIndex: idx
        }));
        if (isNumber(limit) && limit > -1) docs = docs.slice(0, limit);
        return docs;
      }
      const useHeap = shouldSort && isNumber(limit) && limit > 0 && isString(query);
      const comparator = sortFn;
      const stable = (a, b) => comparator(a, b) || a.idx - b.idx;
      let results;
      if (useHeap) {
        const heap = new MaxHeap(limit, stable);
        if (isString(this._docs[0])) this._searchStringList(query, {
          heap,
          ignoreFieldNorm
        });
        else this._searchObjectList(query, {
          heap,
          ignoreFieldNorm
        });
        results = heap.extractSorted();
      } else {
        results = isString(query) ? isString(this._docs[0]) ? this._searchStringList(query) : this._searchObjectList(query) : this._searchLogical(query);
        computeScore(results, { ignoreFieldNorm });
        if (shouldSort) results.sort(isString(query) ? stable : comparator);
        if (isNumber(limit) && limit > -1) results = results.slice(0, limit);
      }
      return format(results, this._docs, {
        includeMatches,
        includeScore
      });
    }
    _searchStringList(query, { heap, ignoreFieldNorm } = {}) {
      const searcher = this._getSearcher(query);
      const requireAllTokens = this.options.useTokenSearch && this.options.tokenMatch === "all";
      const { records } = this._myIndex;
      const results = heap ? null : [];
      records.forEach(({ v: text, i: idx, n: norm2 }) => {
        if (!isDefined(text)) return;
        const searchResult = searcher.searchIn(text);
        if (searchResult.isMatch) {
          const match = {
            score: searchResult.score,
            value: text,
            norm: norm2,
            indices: searchResult.indices
          };
          if (requireAllTokens) {
            match.matchedMask = searchResult.matchedMask;
            match.matchedTerms = searchResult.matchedTerms;
            match.termCount = searchResult.termCount;
          }
          const matches = [match];
          if (!requireAllTokens || this._coversAllTokens(matches)) {
            const result = {
              item: text,
              idx,
              matches
            };
            if (heap) {
              result.score = computeScoreSingle(result.matches, { ignoreFieldNorm });
              heap.insert(result);
            } else results.push(result);
          }
        }
      });
      return results;
    }
    _searchLogical(query) {
      const expression = parse(query, this.options);
      const keys = this._normalizedKeys();
      const evaluate = (node, item, idx) => {
        if (!("children" in node)) {
          const { keyId, searcher } = node;
          let matches;
          if (keyId === null) {
            matches = [];
            keys.forEach((key, keyIndex) => {
              matches.push(...this._findMatches({
                key,
                value: item[keyIndex],
                searcher
              }));
            });
          } else matches = this._findMatches({
            key: this._keyStore.get(keyId),
            value: this._myIndex.getValueForItemAtKeyId(item, keyId),
            searcher
          });
          if (matches && matches.length) return [{
            idx,
            item,
            matches
          }];
          return [];
        }
        const { children, operator } = node;
        const res2 = [];
        for (let i = 0, len = children.length; i < len; i += 1) {
          const child = children[i];
          const result = evaluate(child, item, idx);
          if (result.length) res2.push(...result);
          else if (operator === LogicalOperator.AND) return [];
        }
        return res2;
      };
      const records = this._myIndex.records;
      const resultMap = /* @__PURE__ */ new Map();
      const results = [];
      records.forEach(({ $: item, i: idx }) => {
        if (isDefined(item)) {
          const expResults = evaluate(expression, item, idx);
          if (expResults.length) {
            if (!resultMap.has(idx)) {
              resultMap.set(idx, {
                idx,
                item,
                matches: []
              });
              results.push(resultMap.get(idx));
            }
            expResults.forEach(({ matches }) => {
              resultMap.get(idx).matches.push(...matches);
            });
          }
        }
      });
      return results;
    }
    _searchObjectList(query, { heap, ignoreFieldNorm } = {}) {
      const searcher = this._getSearcher(query);
      const requireAllTokens = this.options.useTokenSearch && this.options.tokenMatch === "all";
      const { records } = this._myIndex;
      const keys = this._normalizedKeys();
      const results = heap ? null : [];
      records.forEach(({ $: item, i: idx }) => {
        if (!isDefined(item)) return;
        const matches = [];
        let anyKeyFailed = false;
        let hasInverse = false;
        keys.forEach((key, keyIndex) => {
          const keyMatches = this._findMatches({
            key,
            value: item[keyIndex],
            searcher
          });
          if (keyMatches.length) {
            matches.push(...keyMatches);
            if (keyMatches[0].hasInverse) hasInverse = true;
          } else anyKeyFailed = true;
        });
        if (hasInverse && anyKeyFailed) return;
        if (matches.length && (!requireAllTokens || this._coversAllTokens(matches))) {
          const result = {
            idx,
            item,
            matches
          };
          if (heap) {
            result.score = computeScoreSingle(result.matches, { ignoreFieldNorm });
            heap.insert(result);
          } else results.push(result);
        }
      });
      return results;
    }
    _findMatches({ key, value, searcher }) {
      if (!isDefined(value)) return [];
      const matches = [];
      if (isArray(value)) value.forEach(({ v: text, i: idx, n: norm2 }) => {
        if (!isDefined(text)) return;
        const searchResult = searcher.searchIn(text);
        if (searchResult.isMatch) {
          const match = {
            score: searchResult.score,
            key,
            value: text,
            idx,
            norm: norm2,
            indices: searchResult.indices,
            hasInverse: searchResult.hasInverse
          };
          if (searchResult.termCount !== void 0) {
            match.matchedMask = searchResult.matchedMask;
            match.matchedTerms = searchResult.matchedTerms;
            match.termCount = searchResult.termCount;
          }
          matches.push(match);
        }
      });
      else {
        const { v: text, n: norm2 } = value;
        const searchResult = searcher.searchIn(text);
        if (searchResult.isMatch) {
          const match = {
            score: searchResult.score,
            key,
            value: text,
            norm: norm2,
            indices: searchResult.indices,
            hasInverse: searchResult.hasInverse
          };
          if (searchResult.termCount !== void 0) {
            match.matchedMask = searchResult.matchedMask;
            match.matchedTerms = searchResult.matchedTerms;
            match.termCount = searchResult.termCount;
          }
          matches.push(match);
        }
      }
      return matches;
    }
    _coversAllTokens(matches) {
      const termCount = matches.length ? matches[0].termCount : void 0;
      if (termCount === void 0) return true;
      if (termCount <= 31) {
        let coverage2 = 0;
        for (let i = 0; i < matches.length; i++) coverage2 |= matches[i].matchedMask || 0;
        return coverage2 === 2 ** termCount - 1;
      }
      const coverage = /* @__PURE__ */ new Set();
      for (let i = 0; i < matches.length; i++) {
        const terms = matches[i].matchedTerms;
        if (terms) for (const t of terms) coverage.add(t);
      }
      return coverage.size === termCount;
    }
  };
  Fuse.version = "7.5.0";
  Fuse.createIndex = createIndex;
  Fuse.parseIndex = parseIndex;
  Fuse.config = Config;
  Fuse.match = function(pattern, text, options) {
    if (options && options.useTokenSearch) throw new Error(FUSE_MATCH_TOKEN_SEARCH_UNSUPPORTED);
    return createSearcher(pattern, {
      ...Config,
      ...options
    }).searchIn(text);
  };
  Fuse.parseQuery = parse;
  register(ExtendedSearch);
  register(TokenSearch);
  Fuse.use = function(...plugins) {
    plugins.forEach((plugin) => register(plugin));
  };
  var entry_default = Fuse;

  // src/engine/search.ts
  var fuse = null;
  var fuseList = [];
  function rebuildFuseIndex(list) {
    fuseList = list;
    fuse = new entry_default(list, {
      keys: [
        { name: "n", weight: 0.45 },
        { name: "d", weight: 0.25 },
        { name: "cat", weight: 0.15 },
        { name: "aliases", weight: 0.15 }
      ],
      threshold: 0.38,
      ignoreLocation: true
    });
  }
  function fuzzyPresets(query, limit = 12) {
    const q = query.trim();
    if (!q) return [];
    if (!fuse) rebuildFuseIndex(fuseList);
    if (!fuse) return [];
    return fuse.search(q, { limit }).map((r) => r.item);
  }
  function parseCategoryFilter(raw) {
    const m = raw.match(/^\s*(web|folder|app|term|apps)\s+(.*)$/i);
    if (!m) return { cat: null, rest: raw.trim() };
    return { cat: m[1].toLowerCase(), rest: m[2].trim() };
  }
  function presetMatchesCat(p, cat) {
    if (!cat) return true;
    if (cat === "apps") return p.cat === "apps" || p.t === "app";
    if (cat === "web") return p.t === "web";
    if (cat === "folder") return p.t === "folder";
    if (cat === "term") return p.t === "term";
    return p.cat.toLowerCase() === cat;
  }
  function findPresetStrict(list, name) {
    const p = name.toLowerCase();
    const exact = list.find((x) => x.n === p) || list.find((x) => (x.aliases ?? []).includes(p));
    if (exact) return exact;
    const prefix = list.filter(
      (x) => x.n.startsWith(p) || (x.aliases ?? []).some((a) => a.startsWith(p))
    );
    if (prefix.length === 1) return prefix[0];
    return void 0;
  }
  function duplicateShortcutIds(list) {
    const seen = /* @__PURE__ */ new Map();
    const dups = [];
    for (const p of list) {
      const keys = [p.n, ...p.aliases ?? []].map((k) => k.toLowerCase());
      for (const k of keys) {
        seen.set(k, (seen.get(k) ?? 0) + 1);
      }
    }
    for (const [k, n] of seen) {
      if (n > 1) dups.push(k);
    }
    return [...new Set(dups)].sort();
  }
  function validatePreset(p) {
    if (!p.n?.trim()) return "shortcut missing name (n)";
    if (!p.t) return `${p.n}: missing type (t)`;
    if (!p.target?.trim()) return `${p.n}: missing target`;
    if (!["web", "folder", "app", "term"].includes(p.t)) {
      return `${p.n}: unknown type '${p.t}'`;
    }
    if (p.t === "web" && !/^https?:\/\//i.test(p.target) && !p.target.includes("://")) {
      return `${p.n}: web target should be a URL`;
    }
    return null;
  }
  function timeAwareBoost(cat, hour) {
    if (cat === "personal" && hour >= 6 && hour < 11) return 3;
    if (cat === "dev" && hour >= 9 && hour < 18) return 2;
    if (cat === "media" && hour >= 17) return 2;
    return 0;
  }
  function orderDefaults(list, recents, pins, opts) {
    const limit = opts.limit ?? 12;
    const hour = (/* @__PURE__ */ new Date()).getHours();
    const byName = new Map(list.map((p) => [p.n, p]));
    const out = [];
    const used = /* @__PURE__ */ new Set();
    const push = (p) => {
      if (!p || used.has(p.n) || p.cat === "apps") return;
      used.add(p.n);
      out.push(p);
    };
    for (const id of pins) push(byName.get(id));
    for (const id of recents) push(byName.get(id));
    const rest = list.filter((p) => p.cat !== "apps" && !used.has(p.n)).map((p) => ({
      p,
      score: timeAwareBoost(p.cat, hour) + (recents.indexOf(p.n) >= 0 ? 5 - recents.indexOf(p.n) : 0)
    })).sort((a, b) => b.score - a.score || a.p.n.localeCompare(b.p.n));
    for (const { p } of rest) {
      push(p);
      if (out.length >= limit) break;
    }
    return out.slice(0, limit);
  }
  function playLaunchTick(enabled) {
    if (!enabled) return;
    try {
      const ctx = new AudioContext();
      const o = ctx.createOscillator();
      const g = ctx.createGain();
      o.connect(g);
      g.connect(ctx.destination);
      o.frequency.value = 880;
      g.gain.value = 0.04;
      o.start();
      g.gain.exponentialRampToValueAtTime(1e-3, ctx.currentTime + 0.08);
      o.stop(ctx.currentTime + 0.09);
    } catch {
    }
  }

  // src/launcher.ts
  var root;
  var titleEl;
  var modeLabel;
  var termWrap;
  var termHost;
  var footL;
  var footM;
  var footHint;
  var footR;
  var inp;
  var psEl;
  var echoEl;
  var sug;
  var res;
  var headDot;
  function mustEl(id) {
    const el = document.getElementById(id);
    if (!el) throw new Error(`Action bar missing #${id}`);
    return el;
  }
  function bindDom() {
    root = mustEl("cli-root");
    titleEl = mustEl("cli-title");
    modeLabel = mustEl("cli-mode-label");
    termWrap = mustEl("cli-term-wrap");
    termHost = mustEl("cli-term");
    footL = mustEl("cli-foot-l");
    footM = mustEl("cli-foot-m");
    footHint = mustEl("cli-foot-r-hint");
    footR = mustEl("cli-foot-r");
    inp = mustEl("cli-in");
    psEl = mustEl("cli-ps");
    echoEl = mustEl("cli-echo");
    sug = mustEl("cli-sug");
    res = mustEl("cli-res");
    headDot = root.querySelector(".cli-head i");
    if (!headDot) throw new Error("Action bar missing .cli-head i");
  }
  var HOME = "";
  var cwd = "";
  var PRESETS = [];
  var USER_SHORTCUTS = [];
  var hist = [];
  var hi = 0;
  var rows = [];
  var rowSel = -1;
  var busy = false;
  var busyGen = 0;
  var blurTimer = null;
  var BLUR_DISMISS_MS = 280;
  var FOCUS_STEAL_GRACE_MS = 320;
  var BUSY_WATCHDOG_MS = 4500;
  var LAUNCH_COOLDOWN_MS = 700;
  function barWindow() {
    return window;
  }
  function launchBlocked() {
    const at = barWindow().__VERSAILLES_LAST_LAUNCH__ || 0;
    return Date.now() - at < LAUNCH_COOLDOWN_MS;
  }
  function claimLaunch() {
    if (launchBlocked()) return false;
    barWindow().__VERSAILLES_LAST_LAUNCH__ = Date.now();
    return true;
  }
  var mode = "action";
  var term = null;
  var fitAddon = null;
  var searchAddon = null;
  var termFindOpen = false;
  var termFindCase = false;
  var termMenuBound = false;
  var ptyDataUnlisten = null;
  var ptyExitUnlisten = null;
  var termSeed = null;
  var sessionAlive = false;
  var ENGINE_ID = "action-bar";
  var ENGINE_OPTS = {
    blurDismissMs: BLUR_DISMISS_MS,
    suggestionLimit: 12,
    compact: false,
    launchTick: false,
    searchHf: "https://huggingface.co/models?search={q}",
    timeAwareDefaults: true,
    autoDismissLaunch: true
  };
  var ENGINE_RUNTIME = { recents: [], pins: [] };
  var lastLaunchError = null;
  var escClearPending = false;
  var termSessionLabel = "";
  var hostAvailable = true;
  var ptyWriteBuf = "";
  var ptyRaf = null;
  var WEB_TLDS = /* @__PURE__ */ new Set([
    "com",
    "org",
    "net",
    "io",
    "dev",
    "app",
    "ai",
    "co",
    "me",
    "tv",
    "gg",
    "to",
    "sh",
    "rs",
    "edu",
    "gov",
    "info",
    "xyz",
    "de",
    "uk",
    "eu",
    "us",
    "ca",
    "au",
    "nl",
    "fr",
    "it",
    "es",
    "ch",
    "at",
    "be",
    "se",
    "no",
    "dk",
    "fi",
    "pl",
    "cz",
    "in",
    "jp",
    "kr",
    "cn",
    "ru",
    "br",
    "mx",
    "nz",
    "ie",
    "pt"
  ]);
  function looksLikeUrl(raw) {
    const s = raw.trim();
    if (!s || /\s/.test(s)) return null;
    if (/^https?:\/\/.+/i.test(s)) return s;
    if (/^www\.[a-z0-9]/i.test(s)) return "https://" + s;
    if (/^localhost(?::\d{1,5})?(?:[/?#].*)?$/i.test(s)) return "http://" + s;
    if (/^\d{1,3}(?:\.\d{1,3}){3}(?::\d{1,5})?(?:[/?#].*)?$/.test(s)) return "http://" + s;
    const host = s.split(/[/?#]/)[0]?.split(":")[0] ?? "";
    const parts = host.split(".");
    if (parts.length < 2) return null;
    const tld = (parts[parts.length - 1] || "").toLowerCase();
    if (!WEB_TLDS.has(tld)) return null;
    if (!/^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]*[a-z0-9])?)+$/i.test(host)) {
      return null;
    }
    return "https://" + s;
  }
  var esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  var joinPath = (a, b) => a.endsWith("\\") ? a + b : a + "\\" + b;
  var psQuote = (s) => `'${s.replace(/'/g, "''")}'`;
  var delay = (ms) => new Promise((r) => setTimeout(r, ms));
  function clearBlurTimer() {
    if (blurTimer) {
      clearTimeout(blurTimer);
      blurTimer = null;
    }
  }
  function forceIdle(_reason) {
    busyGen += 1;
    busy = false;
    clearBlurTimer();
  }
  function scheduleDismiss(ms, reason) {
    clearBlurTimer();
    const genAtSchedule = busyGen;
    blurTimer = setTimeout(() => {
      blurTimer = null;
      if (mode === "terminal") return;
      if (busy || genAtSchedule !== busyGen) {
        scheduleDismiss(ms, reason);
        return;
      }
      void getCurrentWindow().isFocused().then((still) => {
        if (!still && mode === "action" && !busy) dismissAction(reason);
      }).catch(() => {
      });
    }, ms);
  }
  function setPrompt() {
    psEl.textContent = `PS ${cwd}>`;
  }
  function syncEcho() {
    echoEl.textContent = inp.value;
  }
  function setRes(cls, html) {
    res.className = `cli-res on ${cls}`;
    res.innerHTML = html;
  }
  function clearRes() {
    res.className = "cli-res";
    res.innerHTML = "";
  }
  async function withBusy(fn, opts = {}) {
    if (busy) {
      return void 0;
    }
    const focusSteals = opts.focusSteals ?? true;
    const gen = ++busyGen;
    busy = true;
    clearBlurTimer();
    const watchdog = setTimeout(() => {
      if (gen === busyGen && busy) {
        forceIdle("watchdog");
        setRes("err", "command timed out \u2014 dismissed");
        void invoke("dismiss_launcher");
      }
    }, BUSY_WATCHDOG_MS);
    try {
      return await fn();
    } finally {
      clearTimeout(watchdog);
      if (focusSteals) await delay(FOCUS_STEAL_GRACE_MS);
      if (gen === busyGen) {
        busy = false;
        if (mode === "action") void inp.focus();
      }
    }
  }
  function continueRow() {
    const label2 = termSessionLabel ? `reattach \xB7 ${termSessionLabel}` : "reattach background terminal";
    return { c: "continue", d: label2, cc: "continue", live: true };
  }
  function syncLiveChrome() {
    const live = sessionAlive && mode === "action";
    root.classList.toggle("cli-live", live);
    headDot.classList.toggle("cli-live-dot", sessionAlive);
    modeLabel.classList.toggle("cli-live-badge", sessionAlive);
  }
  function showRows(list) {
    rows = list;
    rowSel = -1;
    sug.innerHTML = "";
    rows.forEach((r) => {
      const d = document.createElement("div");
      d.className = r.live ? "cl-s cl-s-live" : "cl-s";
      d.innerHTML = `<b>${esc(r.c)}</b> <span>${esc(r.d || "")}</span>`;
      d.title = r.path || r.d || "";
      d.onmousedown = (e) => {
        e.preventDefault();
        if (e.detail > 1 || launchBlocked()) return;
        activateRow(r);
      };
      sug.appendChild(d);
    });
  }
  function markSel() {
    sug.querySelectorAll(".cl-s").forEach((d, i) => d.classList.toggle("on", i === rowSel));
  }
  function pick(r) {
    if (r.cat && isProfileName(r.cat)) {
      presetRows(r.cat);
      return;
    }
    if (r.cc !== void 0) {
      inp.value = r.cc;
      syncEcho();
      inp.focus();
      refreshProposals();
    } else if (r.path) {
      void openPath(r.path);
    }
  }
  function commandFromRow(r) {
    if (r.path) return null;
    if (r.cat && isProfileName(r.cat)) return null;
    if (r.cc !== void 0 && (r.cc.endsWith(" ") || /<[^>]+>/.test(r.c))) {
      return null;
    }
    return (r.cc ?? r.c).trim() || null;
  }
  function submitCommand(raw, background = false) {
    const v = raw.trim();
    if (busy || launchBlocked()) return;
    escClearPending = false;
    inp.value = "";
    syncEcho();
    rowSel = -1;
    if (v) hist.push(v);
    hi = 0;
    run(v, background);
  }
  function activateRow(r, background = false) {
    if (r.path) {
      rowSel = -1;
      void openPath(r.path);
      return;
    }
    const cmd = commandFromRow(r);
    if (cmd) {
      submitCommand(cmd, background);
      return;
    }
    pick(r);
  }
  function defaults() {
    clearRes();
    const limit = ENGINE_OPTS.suggestionLimit;
    const verbs = [
      { c: "?", d: "search the web", cc: "? " },
      { c: "!!", d: "open a terminal", cc: "!!" }
    ];
    const ordered = orderDefaults(PRESETS, ENGINE_RUNTIME.recents, ENGINE_RUNTIME.pins, {
      timeAware: ENGINE_OPTS.timeAwareDefaults,
      limit
    });
    const rows2 = [];
    const pinSet = new Set(ENGINE_RUNTIME.pins);
    const recentSet = new Set(ENGINE_RUNTIME.recents);
    for (const x of ordered) {
      let tag = x.cat;
      if (pinSet.has(x.n)) tag = `pin \xB7 ${x.cat}`;
      else if (recentSet.has(x.n)) tag = `recent \xB7 ${x.cat}`;
      rows2.push({ c: x.n, d: `${tag} \xB7 ${x.d}`, cc: x.n });
    }
    if (root) root.classList.toggle("cli-compact", ENGINE_OPTS.compact);
    const prefix = sessionAlive ? [continueRow()] : [];
    showRows([...prefix, ...rows2, ...verbs]);
  }
  function findPreset(name) {
    return findPresetStrict(PRESETS, name);
  }
  function presetMatches(x, low) {
    if (!low) return true;
    if (x.n.startsWith(low) || x.n.includes(low) || x.d.toLowerCase().includes(low)) return true;
    return (x.aliases ?? []).some((a) => a.startsWith(low) || a.includes(low));
  }
  function commandForQuery(x, low) {
    if (x.n.startsWith(low)) return x.n;
    const alias = (x.aliases ?? []).find((a) => a.startsWith(low));
    return alias ?? x.n;
  }
  function profileNames() {
    return [...new Set(PRESETS.map((x) => x.cat))].sort((a, b) => b.length - a.length);
  }
  function isProfileName(name) {
    const p = name.toLowerCase();
    return profileNames().some((cat) => cat === p);
  }
  function findInProfile(cat, name) {
    const c = cat.toLowerCase();
    const n = name.toLowerCase().trim();
    if (!n) return void 0;
    const scoped = PRESETS.filter((x) => x.cat.toLowerCase() === c);
    return scoped.find((x) => x.n === n) || scoped.find((x) => (x.aliases ?? []).includes(n)) || scoped.find((x) => x.n.startsWith(n)) || scoped.find((x) => (x.aliases ?? []).some((a) => a.startsWith(n)));
  }
  function suggestions(raw) {
    const s = raw.trim();
    const out = [];
    const typedUrl = looksLikeUrl(s);
    if (typedUrl || /^https?:\/\//i.test(s) || /^www\./i.test(s)) {
      return [{ c: s, d: "open in browser", cc: s }];
    }
    if (/^\?/.test(s)) {
      const q = s.replace(/^\?+\s*/, "");
      const qUrl = looksLikeUrl(q);
      if (qUrl) return [{ c: s, d: "open in browser", cc: s }];
      if (!s.startsWith("??")) out.push({ c: "? " + q, d: "Google search" });
      out.push({ c: "?? " + q, d: "search files" });
      return out;
    }
    const hf = raw.match(/^\s*hf(?:\s+(.*))?$/i);
    if (hf) {
      const q = (hf[1] || "").trim();
      if (!q) {
        out.push({ c: "hf", d: "open Hugging Face" });
        out.push({ c: "hf ", d: "search models \xB7 spaces \xB7 papers" });
      } else {
        out.push({ c: "hf " + q, d: "Hugging Face search" });
      }
      return out;
    }
    const m = raw.match(/^\s*(?:open|o|presets?)(?:\s+(.*))?$/i);
    if (m) {
      const p = (m[1] || "").toLowerCase();
      if (!p || "config".startsWith(p)) out.push({ c: "config", d: "open desktop/index.html" });
      if (!p || "desktopfile".startsWith(p)) out.push({ c: "desktopfile", d: "open desktop/index.html" });
      PRESETS.filter((x) => !p || presetMatches(x, p) || x.cat.toLowerCase().startsWith(p)).slice(0, 8).forEach((x) => out.push({ c: commandForQuery(x, p), d: `${x.cat} \xB7 ${x.d}`, cc: x.n }));
      return out;
    }
    const parts = s.split(/\s+/);
    if (parts.length >= 2 && isProfileName(parts[0].toLowerCase())) {
      const cat = parts[0].toLowerCase();
      const rest = parts.slice(1).join(" ").toLowerCase();
      PRESETS.filter((x) => x.cat.toLowerCase() === cat && (!rest || presetMatches(x, rest))).slice(0, 10).forEach((x) => out.push({ c: commandForQuery(x, rest), d: `${x.cat} \xB7 ${x.d}`, cc: x.n }));
      if (!out.length) out.push({ c: cat, d: `no match in ${cat} \xB7 tab to browse`, cat });
      return out;
    }
    const { cat: catFilter, rest: catRest } = parseCategoryFilter(s);
    if (catFilter && !catRest) {
      PRESETS.filter((x) => presetMatchesCat(x, catFilter)).slice(0, ENGINE_OPTS.suggestionLimit).forEach((x) => out.push({ c: x.n, d: `${x.cat} \xB7 ${x.d}`, cc: x.n }));
      return out;
    }
    const low = (catRest || s).toLowerCase();
    if (low && !s.includes(" ")) {
      if (sessionAlive && ("continue".startsWith(low) || "attach".startsWith(low))) {
        out.push(continueRow());
      }
      if ("config".startsWith(low)) out.push({ c: "config", d: "open desktop/index.html" });
      if ("desktopfile".startsWith(low)) out.push({ c: "desktopfile", d: "open desktop/index.html" });
      if ("term".startsWith(low) || "shell".startsWith(low)) {
        out.push({
          c: sessionAlive ? "term new" : "term",
          d: sessionAlive ? "new terminal (kills background)" : "open embedded terminal"
        });
      }
      if ("lock".startsWith(low)) out.push({ c: "lock", d: "lock workstation" });
      if ("start".startsWith(low)) out.push({ c: "start", d: "Start menu \xB7 installed apps" });
      if ("showdesk".startsWith(low) || "peek".startsWith(low)) {
        out.push({ c: "showdesk", d: "show desktop" });
      }
      if ("desk".startsWith(low)) out.push({ c: "desk", d: "toggle the HTML desktop page" });
      if ("hide".startsWith(low)) out.push({ c: "hide ", d: "hide an auto-added app" });
      profileNames().filter((cat) => cat.startsWith(low)).slice(0, 4).forEach(
        (cat) => out.push({ c: cat, d: `profile \xB7 ${PRESETS.filter((x) => x.cat === cat).length} shortcuts`, cat })
      );
      const fuzzy = fuzzyPresets(low, ENGINE_OPTS.suggestionLimit);
      const list = fuzzy.length ? fuzzy : PRESETS.filter((x) => presetMatches(x, low));
      list.slice(0, ENGINE_OPTS.suggestionLimit).forEach(
        (x) => out.push({
          c: commandForQuery(x, low),
          d: `${x.cat} \xB7 ${x.d} \xB7 ${esc(x.target).slice(0, 48)}`
        })
      );
    }
    return out;
  }
  function refreshProposals() {
    const raw = inp.value;
    if (!raw.trim()) return defaults();
    if (raw.endsWith(" ")) {
      const base = raw.trim().toLowerCase();
      const inCat = PRESETS.filter((x) => x.cat.toLowerCase() === base || x.cat.toLowerCase().startsWith(base));
      if (inCat.length) {
        presetRows(base);
        return;
      }
    }
    const list = suggestions(raw);
    if (list.length) showRows(list.map((x) => ({ ...x, cc: x.cc ?? x.c })));
    else {
      const hint = needsTerminal(raw) ? "\u21B5 open terminal" : "\u21B5 run inline (pwsh)";
      showRows([{ c: raw, d: hint }]);
    }
  }
  function firstLine(s, max = 120) {
    const line = s.trim().split(/\r?\n/, 1)[0] ?? "";
    return line.length > max ? line.slice(0, max) + "\u2026" : line;
  }
  function formatBlock(text, maxLines = 14, maxChars = 2400) {
    const lines = text.trim().split(/\r?\n/).filter(Boolean);
    if (!lines.length) return "";
    const slice = lines.slice(0, maxLines);
    let html = slice.map((l) => esc(l)).join("<br>");
    if (lines.length > maxLines) html += "<br>\u2026";
    if (html.length > maxChars) html = html.slice(0, maxChars) + "\u2026";
    return html;
  }
  function needsTerminal(cmd) {
    const raw = cmd.trim();
    if (/^!!/.test(raw)) return true;
    const low = raw.toLowerCase();
    return /\|\s*(iex|invoke-expression)\b/.test(low) || /\.ps1\b/.test(low) || /\b(read-host|install-module|install-package|winget\s+install|choco\s+install|scoop\s+install)\b/.test(
      low
    );
  }
  function stripTerminalBang(cmd) {
    return cmd.replace(/^!!\s?/, "").trim();
  }
  function stripInlineBang(cmd) {
    return cmd.replace(/^!\s?/, "").trim();
  }
  function b64ToUtf8(b64) {
    const bin = atob(b64);
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    return new TextDecoder("utf-8", { fatal: false }).decode(bytes);
  }
  function flushPtyWriteBuf() {
    ptyRaf = null;
    if (!ptyWriteBuf || !term) {
      ptyWriteBuf = "";
      return;
    }
    const chunk = ptyWriteBuf;
    ptyWriteBuf = "";
    term.write(chunk);
  }
  function enqueuePtyData(text) {
    ptyWriteBuf += text;
    if (ptyRaf == null) {
      ptyRaf = requestAnimationFrame(flushPtyWriteBuf);
    }
  }
  function clearPtyWriteBuf() {
    if (ptyRaf != null) {
      cancelAnimationFrame(ptyRaf);
      ptyRaf = null;
    }
    ptyWriteBuf = "";
  }
  function applyChrome(next) {
    root.dataset.mode = next;
    if (next === "terminal") {
      const label2 = termSessionLabel || "pwsh";
      titleEl.textContent = `versailles \xB7 ${label2}`;
      modeLabel.textContent = "terminal";
      footL.textContent = "alt+space hide";
      footM.textContent = "ctrl+f find";
      footHint.textContent = "right-click session";
      footR.textContent = sessionAlive ? "live \xB7 background ok" : "right-click menu";
    } else {
      titleEl.textContent = "versailles";
      modeLabel.textContent = sessionAlive ? "live" : "actions";
      footL.textContent = "ctrl+l clear";
      footM.textContent = "ctrl+1-9";
      footHint.textContent = "esc";
      const n = PRESETS.filter((x) => x.cat !== "apps").length;
      footR.textContent = n ? `${n} shortcuts \xB7 ? web \xB7 https:// \xB7 !! term` : "? web \xB7 https:// \xB7 !! term \xB7 help";
    }
    syncLiveChrome();
  }
  async function refreshSessionAlive() {
    try {
      sessionAlive = await invoke("pty_is_alive");
    } catch (e) {
      sessionAlive = false;
    }
  }
  async function bindPtyListeners() {
    if (ptyDataUnlisten) {
      ptyDataUnlisten();
      ptyDataUnlisten = null;
    }
    if (ptyExitUnlisten) {
      ptyExitUnlisten();
      ptyExitUnlisten = null;
    }
    ptyDataUnlisten = await listen("pty://data", (ev) => {
      try {
        enqueuePtyData(b64ToUtf8(ev.payload));
      } catch {
      }
    });
    ptyExitUnlisten = await listen("pty://exit", () => {
      sessionAlive = false;
      term?.writeln("\r\n\x1B[90m[session ended \u2014 esc returns to actions]\x1B[0m");
      if (mode === "action") {
        applyChrome("action");
        if (termSessionLabel) {
          setRes("out", "terminal ended");
          const hit = findPreset(termSessionLabel);
          if (hit) showRows([{ c: hit.n, d: `reopen \xB7 ${hit.d}`, cc: hit.n }]);
          else defaults();
        } else defaults();
      }
    });
  }
  var SEARCH_DECO = {
    matchBackground: "#3f3f46",
    matchBorder: "#6b7280",
    matchOverviewRuler: "#6b7280",
    activeMatchBackground: "#f9fafb",
    activeMatchBorder: "#141414",
    activeMatchColorOverviewRuler: "#f9fafb"
  };
  async function copyTermSelection() {
    if (!term?.hasSelection()) return false;
    const text = term.getSelection();
    if (!text) return false;
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      return false;
    }
  }
  async function pasteToTerm() {
    let text = "";
    try {
      text = await navigator.clipboard.readText();
    } catch {
      return false;
    }
    if (!text) return false;
    void invoke("pty_write", { data: text }).catch(() => {
    });
    return true;
  }
  function selectAllTerm() {
    term?.selectAll();
  }
  function clearTermBuffer() {
    term?.clear();
  }
  function searchOpts(incremental = false) {
    return {
      caseSensitive: termFindCase,
      incremental,
      decorations: SEARCH_DECO
    };
  }
  function runTermFind(dir, incremental = false) {
    if (!searchAddon) return;
    const input = document.getElementById("cli-term-find-in");
    const q = input?.value ?? "";
    if (!q) {
      searchAddon.clearDecorations();
      term?.clearSelection();
      return;
    }
    const opts = searchOpts(incremental);
    if (dir === "prev") searchAddon.findPrevious(q, opts);
    else searchAddon.findNext(q, opts);
  }
  function ensureFindBar() {
    if (document.getElementById("cli-term-find")) return;
    const bar = document.createElement("div");
    bar.id = "cli-term-find";
    bar.className = "cli-term-find";
    bar.innerHTML = '<input id="cli-term-find-in" type="search" spellcheck="false" autocomplete="off" placeholder="Find" aria-label="Find in terminal" /><button type="button" id="cli-term-find-prev" title="Previous">\u2191</button><button type="button" id="cli-term-find-next" title="Next">\u2193</button><button type="button" id="cli-term-find-case" title="Match case">Aa</button><button type="button" id="cli-term-find-close" title="Close">\xD7</button>';
    termWrap.appendChild(bar);
    const input = bar.querySelector("#cli-term-find-in");
    const prev = bar.querySelector("#cli-term-find-prev");
    const next = bar.querySelector("#cli-term-find-next");
    const cse = bar.querySelector("#cli-term-find-case");
    const close = bar.querySelector("#cli-term-find-close");
    input.addEventListener("input", () => runTermFind("next", true));
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        e.stopPropagation();
        runTermFind(e.shiftKey ? "prev" : "next");
      } else if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        hideTermFind();
      } else if (e.key.toLowerCase() === "f" && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
      }
    });
    prev.addEventListener("click", () => runTermFind("prev"));
    next.addEventListener("click", () => runTermFind("next"));
    cse.addEventListener("click", () => {
      termFindCase = !termFindCase;
      cse.classList.toggle("on", termFindCase);
      runTermFind("next", true);
    });
    close.addEventListener("click", () => hideTermFind());
    bar.addEventListener("mousedown", (e) => e.stopPropagation());
  }
  function showTermFind() {
    if (!searchAddon) return;
    ensureFindBar();
    const bar = document.getElementById("cli-term-find");
    const input = document.getElementById("cli-term-find-in");
    if (!bar || !input) return;
    bar.classList.add("on");
    termFindOpen = true;
    input.focus();
    input.select();
  }
  function hideTermFind() {
    document.getElementById("cli-term-find")?.classList.remove("on");
    termFindOpen = false;
    searchAddon?.clearDecorations();
    if (mode === "terminal") term?.focus();
  }
  function ensureTermMenu() {
    if (document.getElementById("cli-term-menu")) return;
    const menu = document.createElement("div");
    menu.id = "cli-term-menu";
    menu.className = "cli-term-menu";
    menu.innerHTML = '<button type="button" data-act="copy">Copy</button><button type="button" data-act="paste">Paste</button><button type="button" data-act="select-all">Select All</button><button type="button" data-act="clear">Clear</button><button type="button" data-act="new">New session</button><button type="button" data-act="kill">Kill session</button>';
    document.body.appendChild(menu);
    menu.addEventListener("mousedown", (e) => e.stopPropagation());
    menu.addEventListener("click", (e) => {
      const btn = e.target.closest("button");
      if (!btn) return;
      const act = btn.getAttribute("data-act");
      hideTermMenu();
      if (act === "copy") void copyTermSelection();
      else if (act === "paste") void pasteToTerm();
      else if (act === "select-all") selectAllTerm();
      else if (act === "clear") clearTermBuffer();
      else if (act === "new") {
        forceIdle("term-new");
        void enterTerminal(void 0, { fresh: true });
      } else if (act === "kill") {
        void closeTerminalFromCommand();
      }
    });
    if (!termMenuBound) {
      termMenuBound = true;
      document.addEventListener("mousedown", (e) => {
        if (!menu.classList.contains("on")) return;
        if (menu.contains(e.target)) return;
        hideTermMenu();
      });
    }
  }
  function showTermMenu(x, y) {
    ensureTermMenu();
    const menu = document.getElementById("cli-term-menu");
    if (!menu) return;
    const copyBtn = menu.querySelector('[data-act="copy"]');
    if (copyBtn) copyBtn.disabled = !term?.hasSelection();
    menu.classList.add("on");
    const pad = 8;
    const w = menu.offsetWidth || 148;
    const h = menu.offsetHeight || 140;
    menu.style.left = `${Math.min(x, window.innerWidth - w - pad)}px`;
    menu.style.top = `${Math.min(y, window.innerHeight - h - pad)}px`;
  }
  function hideTermMenu() {
    document.getElementById("cli-term-menu")?.classList.remove("on");
  }
  function bindTermChrome(t) {
    t.attachCustomKeyEventHandler((ev) => {
      if (ev.type !== "keydown") return true;
      const key = ev.key.toLowerCase();
      const mod = ev.ctrlKey || ev.metaKey;
      if (key === "escape") return true;
      if (ev.shiftKey && ev.key === "PageUp") {
        t.scrollPages(-1);
        return false;
      }
      if (ev.shiftKey && ev.key === "PageDown") {
        t.scrollPages(1);
        return false;
      }
      if (mod && key === "f") {
        showTermFind();
        return false;
      }
      if (mod && ev.shiftKey && key === "c") {
        void copyTermSelection();
        return false;
      }
      if (mod && key === "c" && t.hasSelection()) {
        void copyTermSelection();
        return false;
      }
      if (mod && key === "v") {
        void pasteToTerm();
        return false;
      }
      if (ev.shiftKey && key === "insert") {
        void pasteToTerm();
        return false;
      }
      if (ev.ctrlKey && key === "insert") {
        void copyTermSelection();
        return false;
      }
      return true;
    });
    termHost.addEventListener(
      "contextmenu",
      (e) => {
        if (mode !== "terminal") return;
        e.preventDefault();
        e.stopPropagation();
        showTermMenu(e.clientX, e.clientY);
      },
      true
    );
    termHost.addEventListener("paste", (e) => {
      if (mode !== "terminal") return;
      const text = e.clipboardData?.getData("text");
      if (!text) return;
      e.preventDefault();
      void invoke("pty_write", { data: text }).catch(() => {
      });
    });
  }
  function ensureTerm() {
    if (term?.options.convertEol) {
      try {
        term.dispose();
      } catch {
      }
      term = null;
      fitAddon = null;
      searchAddon = null;
    }
    if (term) return term;
    const t = new Terminal({
      // ConPTY already translates newlines — convertEol wraps TUIs onto line 1.
      convertEol: false,
      cursorBlink: true,
      cursorStyle: "bar",
      fontFamily: '"Cascadia Mono", "JetBrains Mono", Consolas, monospace',
      fontSize: 13,
      lineHeight: 1,
      scrollback: 1e4,
      scrollSensitivity: 1,
      smoothScrollDuration: 0,
      fastScrollModifier: "alt",
      fastScrollSensitivity: 5,
      drawBoldTextInBrightColors: true,
      minimumContrastRatio: 4.5,
      wordSeparator: ' ()[]{}\',"`"',
      windowsPty: { backend: "conpty", buildNumber: 26100 },
      overviewRulerWidth: 0,
      allowProposedApi: true,
      theme: {
        background: "#131212",
        foreground: "#f9fafb",
        cursor: "#f9fafb",
        cursorAccent: "#131212",
        selectionBackground: "rgba(249, 250, 251, 0.18)",
        black: "#1c1c1e",
        red: "#ff6b62",
        green: "#30d158",
        yellow: "#ffd60a",
        blue: "#64d2ff",
        magenta: "#bf5af2",
        cyan: "#64d2ff",
        white: "#f2f2f7",
        brightBlack: "#636366",
        brightRed: "#ff6961",
        brightGreen: "#9ac324",
        brightYellow: "#ffd426",
        brightBlue: "#70d7ff",
        brightMagenta: "#da8fff",
        brightCyan: "#70d7ff",
        brightWhite: "#ffffff"
      }
    });
    const fit = new FitAddon();
    t.loadAddon(fit);
    try {
      const search2 = new SearchAddon();
      t.loadAddon(search2);
      searchAddon = search2;
    } catch {
      searchAddon = null;
    }
    try {
      t.loadAddon(
        new WebLinksAddon((_ev, uri) => {
          void invoke("cli_open", { target: uri }).catch(() => {
          });
        })
      );
    } catch {
    }
    t.open(termHost);
    fitAddon = fit;
    term = t;
    if (typeof ResizeObserver !== "undefined") {
      let fitTimer = 0;
      new ResizeObserver(() => {
        if (mode !== "terminal") return;
        window.clearTimeout(fitTimer);
        fitTimer = window.setTimeout(() => {
          void fitAndResizePty();
        }, 40);
      }).observe(termHost);
    }
    t.onData((data) => {
      void invoke("pty_write", { data }).catch(() => {
      });
    });
    bindTermChrome(t);
    return t;
  }
  async function resizeLauncher(next) {
    const win = getCurrentWindow();
    try {
      const monitor = await currentMonitor();
      const scale = monitor?.scaleFactor ?? 1;
      const monW = monitor ? monitor.size.width / scale : 1920;
      const monH = monitor ? monitor.size.height / scale : 1080;
      let w;
      let h;
      if (next === "terminal") {
        w = Math.round(Math.min(1280, Math.max(960, monW * 0.78)));
        h = Math.round(Math.min(860, Math.max(640, monH * 0.78)));
      } else {
        w = 640;
        h = 420;
      }
      await win.setSize(new LogicalSize(w, h));
      document.body.style.width = "";
      document.body.style.height = "";
      if (monitor) {
        const x = monitor.position.x + Math.round((monitor.size.width - w * scale) / 2);
        const y = next === "terminal" ? monitor.position.y + Math.round((monitor.size.height - h * scale) / 2) : monitor.position.y + Math.round(120 * scale);
        await win.setPosition(new PhysicalPosition(x, y));
      }
    } catch {
      await win.setSize(
        next === "terminal" ? new LogicalSize(1100, 720) : new LogicalSize(640, 420)
      );
    }
  }
  function fitTermExact() {
    if (!term || !fitAddon) return;
    const proposed = fitAddon.proposeDimensions();
    if (!proposed || !proposed.cols || !proposed.rows) {
      fitAddon.fit();
      return;
    }
    const style = getComputedStyle(termHost);
    const padX = (parseFloat(style.paddingLeft) || 0) + (parseFloat(style.paddingRight) || 0);
    const cellW = proposed.cols > 0 && termHost.clientWidth > 0 ? termHost.clientWidth / proposed.cols : 9;
    const padCols = cellW > 1 && padX > 0 ? Math.ceil(padX / cellW) : 0;
    const cols = Math.max(20, proposed.cols - padCols - 1);
    const rows2 = Math.max(8, proposed.rows);
    if (term.cols !== cols || term.rows !== rows2) {
      term.resize(cols, rows2);
    }
  }
  async function fitAndResizePty() {
    if (!term || !fitAddon) return;
    fitTermExact();
    const cols = term.cols;
    const rows2 = term.rows;
    try {
      await invoke("pty_resize", { cols, rows: rows2 });
    } catch {
    }
  }
  async function settleTermSize() {
    fitTermExact();
    await new Promise((r) => requestAnimationFrame(() => r()));
    fitTermExact();
    await delay(60);
    await fitAndResizePty();
  }
  function termSizeReady(t) {
    return termHost.clientWidth > 0 && termHost.clientHeight > 0 && (t.cols || 0) >= 20 && (t.rows || 0) >= 8;
  }
  async function waitForTermSize(t) {
    for (let i = 0; i < 16; i++) {
      fitTermExact();
      if (termSizeReady(t)) {
        return { cols: t.cols, rows: t.rows };
      }
      await delay(40);
    }
    fitTermExact();
    return { cols: Math.max(20, t.cols || 80), rows: Math.max(8, t.rows || 24) };
  }
  async function detachTerminal(focusAction = true) {
    if (mode !== "terminal") {
      if (focusAction) {
        await refreshSessionAlive();
        applyChrome("action");
        defaults();
        inp.focus();
      }
      return;
    }
    clearBlurTimer();
    hideTermFind();
    hideTermMenu();
    mode = "action";
    applyChrome("action");
    termWrap.hidden = true;
    try {
      await resizeLauncher("action");
    } catch {
    }
    await refreshSessionAlive();
    if (focusAction) {
      defaults();
      inp.focus();
    }
  }
  async function killTerminalSession() {
    try {
      await invoke("pty_close");
    } catch {
    }
    sessionAlive = false;
    if (ptyDataUnlisten) {
      ptyDataUnlisten();
      ptyDataUnlisten = null;
    }
    if (ptyExitUnlisten) {
      ptyExitUnlisten();
      ptyExitUnlisten = null;
    }
    if (term) {
      term.reset();
    }
  }
  async function closeTerminalFromCommand() {
    forceIdle("term-kill");
    await killTerminalSession();
    termSessionLabel = "";
    try {
      await detachTerminal(false);
    } catch {
      mode = "action";
      termWrap.hidden = true;
    }
    await refreshSessionAlive();
    applyChrome("action");
    defaults();
    setRes("ok", "&rarr; background terminal closed");
    inp.focus();
  }
  async function enterTerminal(seedCmd, opts = {}) {
    const fresh = opts.fresh === true;
    termSeed = seedCmd?.trim() || null;
    clearBlurTimer();
    void invoke("arm_overlay_focus_guard", { ms: 800 }).catch(() => {
    });
    await withBusy(async () => {
      try {
        await refreshSessionAlive();
        if (fresh && sessionAlive) {
          await killTerminalSession();
        }
        if (mode === "terminal" && sessionAlive && !fresh) {
          if (termSeed) {
            const payload = termSeed.endsWith("\n") ? termSeed : termSeed + "\r";
            await invoke("pty_write", { data: payload });
            termSeed = null;
          }
          term?.focus();
          return;
        }
        if (sessionAlive && !fresh) {
          mode = "terminal";
          applyChrome("terminal");
          termWrap.hidden = false;
          await resizeLauncher("terminal");
          await delay(80);
          ensureTerm();
          if (!ptyDataUnlisten) await bindPtyListeners();
          await settleTermSize();
          term?.focus();
          if (termSeed) {
            const payload = termSeed.endsWith("\n") ? termSeed : termSeed + "\r";
            await invoke("pty_write", { data: payload });
            termSeed = null;
          }
          return;
        }
        mode = "terminal";
        applyChrome("terminal");
        termWrap.hidden = false;
        await resizeLauncher("terminal");
        await delay(80);
        const t = ensureTerm();
        t.reset();
        clearPtyWriteBuf();
        const size = await waitForTermSize(t);
        await bindPtyListeners();
        await invoke("pty_open", { cwd, cols: size.cols, rows: size.rows });
        sessionAlive = true;
        await settleTermSize();
        t.focus();
        if (termSeed) {
          await delay(180);
          const payload = termSeed.endsWith("\n") ? termSeed : termSeed + "\r";
          await invoke("pty_write", { data: payload });
          termSeed = null;
        }
      } catch (e) {
        forceIdle("enterTerminal-error");
        setRes("err", esc(String(e)));
        sessionAlive = false;
        try {
          await detachTerminal(false);
        } catch {
          mode = "action";
          termWrap.hidden = true;
          applyChrome("action");
        }
      }
    }, { focusSteals: false });
  }
  async function openPath(path) {
    if (!claimLaunch()) return;
    await withBusy(async () => {
      try {
        await invoke("cli_open", { target: path });
        setRes("ok", `&rarr; opened <span class="link">${esc(path)}</span>`);
      } catch (e) {
        setRes("err", esc(String(e)));
      }
    });
  }
  function webSearch(q) {
    if (!q) return setRes("err", "? : missing query \u2014 usage: ? &lt;query&gt;");
    const url = looksLikeUrl(q);
    if (url) return openTarget(url);
    openTarget("https://www.google.com/search?q=" + encodeURIComponent(q));
  }
  function openTarget(target, opts = {}) {
    if (!claimLaunch()) return;
    void withBusy(async () => {
      try {
        if (!hostAvailable) throw new Error("host unavailable \u2014 preview mode cannot launch");
        await invoke("cli_open", { target });
        playLaunchTick(ENGINE_OPTS.launchTick);
        setRes("ok", `&rarr; opened <span class="link">${esc(target)}</span>`);
        if (ENGINE_OPTS.autoDismissLaunch && !opts.background) {
          await delay(280);
          await hideLauncherWindow("launch");
        }
      } catch (e) {
        setRes("err", esc(String(e)));
      }
    }, { focusSteals: false });
  }
  function calcExpr(expr) {
    if (!expr) return setRes("err", "= : missing expression \u2014 usage: = 1+2*3");
    const safe = expr.replace(/,/g, ".");
    if (!/^[0-9+\-*/().\s%^]*$/.test(safe)) {
      return setRes("err", "= : only numbers and + - * / % ^ ( ) allowed");
    }
    void withBusy(async () => {
      try {
        const ps = safe.replace(/\^/g, "**");
        const out = await invoke("cli_exec", {
          cmd: `[math]::Round((${ps}), 6)`,
          cwd
        });
        if (out.code !== 0 || out.stderr.trim()) {
          setRes("err", esc(firstLine(out.stderr) || "calc failed"));
        } else {
          setRes("ok", `${esc(safe)} = <b>${esc(out.stdout.trim())}</b>`);
        }
      } catch (e) {
        setRes("err", esc(String(e)));
      }
    }, { focusSteals: false });
  }
  function wikiSearch(q) {
    if (!q) return setRes("err", "w : missing query \u2014 usage: w &lt;topic&gt;");
    openTarget("https://en.wikipedia.org/wiki/Special:Search?search=" + encodeURIComponent(q));
  }
  function ytSearch(q) {
    if (!q) return setRes("err", "yt : missing query \u2014 usage: yt &lt;query&gt;");
    openTarget("https://www.youtube.com/results?search_query=" + encodeURIComponent(q));
  }
  function ghSearch(q) {
    if (!q) return openTarget("https://github.com/search");
    openTarget("https://github.com/search?q=" + encodeURIComponent(q));
  }
  function hfSearch(q) {
    const raw = q.trim();
    const tpl = root?.getAttribute("data-search-hf") || "https://huggingface.co/models?search={q}";
    if (!raw) return openTarget("https://huggingface.co/models");
    const parts = raw.split(/\s+/);
    const kind = (parts[0] || "").toLowerCase();
    const rest = parts.slice(1).join(" ").trim();
    if (["models", "datasets", "spaces", "papers"].includes(kind)) {
      if (!rest) {
        return openTarget(`https://huggingface.co/${kind}`);
      }
      if (kind === "papers") {
        return openTarget("https://huggingface.co/papers?q=" + encodeURIComponent(rest));
      }
      return openTarget(`https://huggingface.co/${kind}?search=` + encodeURIComponent(rest));
    }
    openTarget(tpl.replaceAll("{q}", encodeURIComponent(raw)));
  }
  async function fileSearch(q) {
    if (!q) return setRes("err", "?? : missing query \u2014 usage: ?? &lt;query&gt;");
    setRes("out", `searching files for "${esc(q)}"\u2026`);
    await withBusy(async () => {
      try {
        const matches = await invoke("cli_search_files", { query: q });
        if (!matches.length) {
          setRes("out", `no files match "${esc(q)}"`);
          return defaults();
        }
        setRes("out", `${matches.length} match${matches.length > 1 ? "es" : ""} for "${esc(q)}"`);
        showRows(matches.slice(0, 6).map((p) => ({ c: p, path: p })));
      } catch (e) {
        setRes("err", esc(String(e)));
      }
    }, { focusSteals: false });
  }
  function presetRows(filter) {
    const f = (filter || "").toLowerCase();
    const list = f ? PRESETS.filter((x) => presetMatches(x, f) || x.cat.toLowerCase().includes(f)) : PRESETS;
    if (!list.length) {
      setRes("err", `presets : nothing matches '${esc(filter || "")}'`);
      return defaults();
    }
    const cats = [...new Set(list.map((x) => x.cat))];
    setRes("out", f ? `${list.length} in \u201C${esc(f)}\u201D` : `${list.length} shortcuts \xB7 ${cats.join(" \xB7 ")}`);
    showRows(list.map((x) => ({ c: x.n, d: `${x.cat} \xB7 ${x.d}`, cc: x.n })));
  }
  async function openFileTarget(target, label2) {
    if (!claimLaunch()) return;
    await withBusy(async () => {
      try {
        await invoke("cli_open", { target });
        setRes("ok", `&rarr; opened <span class="link">${esc(label2)}</span>`);
        await hideLauncherWindow("openFileTarget");
      } catch (e) {
        setRes("err", esc(String(e)));
      }
    }, { focusSteals: false });
  }
  async function openPreset(arg) {
    const p = arg.toLowerCase().trim();
    if (!p) return presetRows();
    if (p === "config") return void openConfigFile();
    if (p === "desktopfile") return void openDesktopFile();
    const hit = findPreset(p);
    if (!hit) {
      const inCat = PRESETS.filter((x) => x.cat.toLowerCase() === p || x.cat.toLowerCase().startsWith(p));
      if (inCat.length > 1) return presetRows(p);
      if (inCat.length === 1) {
        return void launchPreset(inCat[0]);
      }
      return setRes("err", `no shortcut '${esc(arg)}' \u2014 try 'presets'`);
    }
    return void launchPreset(hit);
  }
  function widgetsRoot(home) {
    return joinPath(home, "Documents\\Widgets");
  }
  async function openConfigFile() {
    return openDesktopFile();
  }
  async function openDesktopFile() {
    return openFileTarget(joinPath(widgetsRoot(HOME || "C:\\"), "desktop\\index.html"), "desktop/index.html");
  }
  async function launchPreset(hit, opts = {}) {
    if (hit.t === "term") {
      clearBlurTimer();
      termSessionLabel = hit.n;
      void saveLastTermSeed(ENGINE_ID, hit.target);
      return void enterTerminal(hit.target, { fresh: true });
    }
    lastLaunchError = null;
    if (!claimLaunch()) return;
    await withBusy(async () => {
      try {
        if (!hostAvailable) throw new Error("host unavailable \u2014 preview mode cannot launch");
        await invoke("cli_open", { target: hit.target });
        void pushRecent(ENGINE_ID, hit.n);
        playLaunchTick(ENGINE_OPTS.launchTick);
        if (hit.t === "web") {
          setRes("ok", `&rarr; opened <span class="link">${esc(hit.d)}</span>`);
        } else if (hit.t === "folder") {
          setRes("ok", `&rarr; opening <span class="link">${esc(hit.d)}</span> in Explorer`);
        } else {
          setRes("ok", `&rarr; launching ${esc(hit.d)}`);
        }
        if (ENGINE_OPTS.autoDismissLaunch && !opts.background) {
          await delay(280);
          await hideLauncherWindow("launch");
        }
      } catch (e) {
        const msg = String(e);
        lastLaunchError = { preset: hit, err: msg };
        setRes("err", esc(msg));
        showRows([
          { c: `retry ${hit.n}`, d: "try again", cc: hit.n },
          { c: "config", d: "edit shortcuts in index.html", cc: "config" }
        ]);
      }
    }, { focusSteals: false });
  }
  function expandHome(target, home) {
    return target.replace(/^~([\\/]|$)/, home + "$1").replace(/%HOME%/gi, home).replace(/\$HOME/g, home);
  }
  function normalizeUserShortcut(raw, home) {
    return {
      ...raw,
      target: raw.t === "folder" ? expandHome(raw.target, home) : raw.target
    };
  }
  function mergePresetsInto(base, extras) {
    const result = [...base];
    const byId = new Map(result.map((p, i) => [p.n, i]));
    for (const p of extras) {
      const idx = byId.get(p.n);
      if (idx !== void 0) {
        result[idx] = p;
      } else {
        byId.set(p.n, result.length);
        result.push(p);
      }
    }
    return result;
  }
  function parseVersaillesBlock(html) {
    try {
      const doc = new DOMParser().parseFromString(html, "text/html");
      const el = doc.getElementById("versailles");
      if (!el) return null;
      return JSON.parse(el.textContent || "");
    } catch {
      return null;
    }
  }
  async function loadUserShortcuts() {
    USER_SHORTCUTS = [];
    const home = HOME || "C:\\";
    try {
      let html = "";
      try {
        html = await invoke("get_desktop_html");
      } catch {
        const api2 = await invoke("get_api_info");
        const res3 = await fetch(`${api2.base_url}/files/desktop/index.html`, { cache: "no-store" });
        if (res3.ok) html = await res3.text();
      }
      const inline = window.__VERSAILLES_BLOCK__;
      if (inline && Array.isArray(inline.shortcuts)) {
        USER_SHORTCUTS = inline.shortcuts.map((s) => normalizeUserShortcut(s, home));
        return;
      }
      const block = html ? parseVersaillesBlock(html) : null;
      if (block && Array.isArray(block.shortcuts)) {
        USER_SHORTCUTS = block.shortcuts.map((s) => normalizeUserShortcut(s, home));
        return;
      }
      const api = await invoke("get_api_info");
      const base = api.base_url;
      let shortcutsPath = "shortcuts.json";
      try {
        const cfgRes = await fetch(`${base}/files/versailles.json`, { cache: "no-store" });
        if (cfgRes.ok) {
          const cfg = await cfgRes.json();
          if (Array.isArray(cfg.shortcuts)) {
            USER_SHORTCUTS = cfg.shortcuts.map((s) => normalizeUserShortcut(s, home));
            return;
          }
          if (typeof cfg.shortcuts === "string" && cfg.shortcuts.trim()) {
            shortcutsPath = cfg.shortcuts.replace(/\\/g, "/").replace(/^\//, "");
          }
        }
      } catch {
      }
      const res2 = await fetch(`${base}/files/${shortcutsPath}`, { cache: "no-store" });
      if (!res2.ok) return;
      const data = await res2.json();
      if (!Array.isArray(data.shortcuts)) return;
      USER_SHORTCUTS = data.shortcuts.map((s) => normalizeUserShortcut(s, home));
    } catch {
    }
  }
  function builtinPresets(home) {
    return [
      { n: "mail", t: "web", d: "Gmail", target: "https://mail.google.com/", cat: "personal" },
      { n: "github", t: "web", d: "GitHub", target: "https://github.com/", cat: "dev" },
      { n: "downloads", t: "folder", d: "Downloads", target: joinPath(home, "Downloads"), cat: "folders" },
      { n: "documents", t: "folder", d: "Documents", target: joinPath(home, "Documents"), cat: "folders" },
      { n: "desktop", t: "folder", d: "Desktop", target: joinPath(home, "Desktop"), cat: "folders" }
    ];
  }
  function applyCatalog(entries) {
    const builtins = mergePresetsInto(builtinPresets(HOME || "C:\\"), USER_SHORTCUTS);
    const byId = new Map(builtins.map((p, i) => [p.n, i]));
    for (const e of entries) {
      const preset = {
        n: e.id,
        t: "app",
        d: e.fresh ? `new \xB7 ${e.name}` : e.name,
        target: e.target,
        cat: "apps",
        aliases: (e.aliases || []).filter((a) => a !== e.id)
      };
      const idx = byId.get(e.id);
      if (idx !== void 0 && builtins[idx].t === "app") {
        builtins[idx] = preset;
      } else if (idx === void 0) {
        builtins.push(preset);
        byId.set(e.id, builtins.length - 1);
      }
    }
    PRESETS = builtins;
  }
  async function loadCatalog() {
    try {
      const entries = await invoke("list_catalog");
      applyCatalog(entries);
      void invoke("ack_catalog").catch(() => {
      });
      return entries.some((e) => e.fresh);
    } catch {
      applyCatalog([]);
      return false;
    }
  }
  async function refreshPresets() {
    await loadUserShortcuts();
    const dups = duplicateShortcutIds(USER_SHORTCUTS);
    const bad = USER_SHORTCUTS.map((p) => validatePreset(p)).filter(Boolean);
    if (dups.length || bad.length) {
      const msg = [
        dups.length ? `duplicate: ${dups.slice(0, 4).join(", ")}` : "",
        bad.length ? bad.slice(0, 2).join(" \xB7 ") : ""
      ].filter(Boolean).join(" \xB7 ");
      setRes("err", esc(msg));
    }
    rebuildFuseIndex(PRESETS.length ? PRESETS : USER_SHORTCUTS);
    ENGINE_RUNTIME = await loadEngineRuntime(ENGINE_ID);
    return loadCatalog();
  }
  async function cdCmd(arg) {
    if (!arg || arg === "~") {
      cwd = HOME;
      setPrompt();
      return setRes("out", esc(cwd));
    }
    const p = arg.replace(/\//g, "\\");
    let target;
    if (/^[A-Za-z]:[\\/]/.test(p)) {
      target = p;
    } else {
      const stack = [];
      (cwd + "\\" + p).split("\\").forEach((seg) => {
        if (seg === "..") stack.pop();
        else if (seg && seg !== ".") stack.push(seg);
      });
      target = stack.join("\\");
    }
    target = target.replace(/\\+$/, "");
    await withBusy(async () => {
      try {
        const out = await invoke("cli_exec", {
          cmd: `if (Test-Path -LiteralPath ${psQuote(target)} -PathType Container) { Write-Output 'OK' }`,
          cwd
        });
        if (out.stdout.trim() === "OK") {
          cwd = target;
          setPrompt();
          setRes("out", esc(cwd));
        } else {
          setRes("err", `cd : Cannot find path '${esc(target)}' because it does not exist.`);
        }
      } catch (e) {
        setRes("err", esc(String(e)));
      }
    }, { focusSteals: false });
  }
  async function lsCmd() {
    await withBusy(async () => {
      try {
        const out = await invoke("cli_exec", {
          cmd: "Get-ChildItem -Force | ForEach-Object { if ($_.PSIsContainer) { 'D|' + $_.Name } else { 'F|' + $_.Name } }",
          cwd
        });
        if (out.code !== 0) return setRes("err", esc(firstLine(out.stderr) || "ls failed"));
        const entries = out.stdout.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
        const dirs = entries.filter((e) => e.startsWith("D|")).map((e) => e.slice(2));
        const files = entries.filter((e) => e.startsWith("F|")).map((e) => e.slice(2));
        const n = dirs.length + files.length;
        if (!n) {
          setRes("out", "(empty) \u2014 " + esc(cwd));
          return defaults();
        }
        setRes("out", `${n} entr${n === 1 ? "y" : "ies"} \u2014 ${esc(cwd)}`);
        showRows(
          [
            ...dirs.map((d) => ({ c: d + "/", d: "dir", cc: "cd " + d })),
            ...files.map((f) => ({ c: f, d: "file", path: joinPath(cwd, f) }))
          ].slice(0, 6)
        );
      } catch (e) {
        setRes("err", esc(String(e)));
      }
    }, { focusSteals: false });
  }
  async function shellExec(cmd) {
    const trimmed = cmd.trim();
    if (/^!!/.test(trimmed)) {
      const run2 = stripTerminalBang(trimmed);
      return void enterTerminal(run2 || void 0);
    }
    const inline = /^!\s?/.test(trimmed) ? stripInlineBang(trimmed) : trimmed;
    if (!inline) return setRes("err", "! : missing command \u2014 usage: ! Get-Date");
    if (needsTerminal(trimmed) && !/^!\s?/.test(trimmed)) {
      return void enterTerminal(inline);
    }
    setRes("out", "\u2026");
    await withBusy(async () => {
      try {
        const out = await invoke("cli_exec", { cmd: inline, cwd });
        if (out.code === 0 && !out.stderr.trim()) {
          const block = formatBlock(out.stdout);
          if (block) setRes("out", block);
          else setRes("ok", "&rarr; done (exit 0)");
        } else {
          const block = formatBlock(out.stderr || out.stdout);
          if (block) setRes("err", block);
          else setRes("err", esc(`exit ${out.code}`));
        }
      } catch (e) {
        setRes("err", esc(String(e)));
      }
    }, { focusSteals: false });
  }
  function run(c, background = false) {
    const bg = { background };
    if (!c) return;
    if (background && c.includes(" ")) {
      const parts = c.split(/\s+/);
      void (async () => {
        for (const name of parts) {
          const hit = findPreset(name);
          if (hit) await launchPreset(hit, { background: true });
        }
      })();
      return;
    }
    if (c === "cls" || c === "clear") {
      clearRes();
      return defaults();
    }
    if (c.startsWith("??")) return void fileSearch(c.slice(2).trim());
    if (c.startsWith("?")) return void webSearch(c.slice(1).trim());
    if (c.startsWith("=")) return void calcExpr(c.slice(1).trim());
    if (c.startsWith("!!") || c.startsWith("!")) return void shellExec(c);
    const typedUrl = looksLikeUrl(c);
    if (typedUrl) return openTarget(typedUrl);
    const sp = c.split(/\s+/);
    const cmd = sp[0].toLowerCase();
    const arg = sp.slice(1).join(" ");
    switch (cmd) {
      case "config":
        return void openConfigFile();
      case "desktopfile":
        return void openDesktopFile();
      case "open":
      case "o":
        return void openPreset(arg);
      case "presets":
      case "shortcuts":
        return presetRows(arg);
      case "continue":
      case "attach":
        return void enterTerminal();
      case "term":
      case "shell":
      case "ps": {
        const sub = arg.trim().toLowerCase();
        if (sub === "new" || sub === "fresh") {
          forceIdle("term-new");
          return void enterTerminal(void 0, { fresh: true });
        }
        if (sub === "kill" || sub === "close") {
          return void closeTerminalFromCommand();
        }
        return void enterTerminal();
      }
      case "w":
      case "wiki":
        return void wikiSearch(arg);
      case "yt":
      case "youtube":
        if (!arg) {
          const hit = findPreset(cmd);
          if (hit) return void launchPreset(hit);
        }
        return void ytSearch(arg);
      case "gh":
        if (!arg) {
          const hit = findPreset(cmd) || findPreset("github");
          if (hit) return void launchPreset(hit);
        }
        return void ghSearch(arg);
      case "hf":
        if (!arg) {
          const hit = findPreset(cmd) || findPreset("huggingface");
          if (hit) return void launchPreset(hit);
        }
        return void hfSearch(arg);
      case "lock":
        return void withBusy(async () => {
          try {
            await invoke("cli_exec", { cmd: "rundll32.exe user32.dll,LockWorkStation", cwd });
            setRes("ok", "&rarr; locked");
          } catch (e) {
            setRes("err", esc(String(e)));
          }
        }, { focusSteals: false });
      case "apps":
        return presetRows("apps");
      case "start":
        if (!arg) return presetRows("apps");
        return void openPreset(arg);
      case "showdesk":
      case "peek":
        return void withBusy(async () => {
          try {
            await invoke("shell_show_desktop");
            setRes("ok", "&rarr; show desktop");
          } catch (e) {
            setRes("err", esc(String(e)));
          }
        });
      case "desk":
        return void withBusy(async () => {
          try {
            const on = await invoke("toggle_desktop_surface");
            setRes("ok", on ? "&rarr; desktop page" : "&rarr; closed desktop");
            if (on) await hideLauncherWindow("desk");
          } catch (e) {
            setRes("err", esc(String(e)));
          }
        }, { focusSteals: false });
      case "hide": {
        const id = arg.trim().toLowerCase();
        if (!id) return setRes("err", "hide : usage hide &lt;app&gt;");
        const hit = findPreset(id);
        const targetId = hit?.t === "app" ? hit.n : id;
        return void withBusy(async () => {
          try {
            const entries = await invoke("hide_catalog_entry", { id: targetId });
            applyCatalog(entries);
            setRes("ok", `&rarr; hid ${esc(targetId)} from apps`);
            presetRows("apps");
          } catch (e) {
            setRes("err", esc(String(e)));
          }
        }, { focusSteals: false });
      }
      case "help":
        setRes("out", "type a shortcut name \xB7 or one of these");
        return showRows([
          ...sessionAlive ? [continueRow()] : [],
          { c: "?", d: "search the web", cc: "? " },
          { c: "https://", d: "open a URL", cc: "https://" },
          { c: "??", d: "search files", cc: "?? " },
          { c: "!!", d: "open a terminal", cc: "!!" },
          { c: "!", d: "run pwsh inline", cc: "! " },
          { c: "=", d: "calculator", cc: "= " },
          { c: "start", d: "installed apps", cc: "start" },
          { c: "desk", d: "toggle the desktop page", cc: "desk" },
          { c: "config", d: "edit index.html", cc: "config" },
          { c: "lock", d: "lock workstation", cc: "lock" }
        ]);
      case "pwd":
      case "get-location":
        return setRes("out", esc(cwd));
      case "cd":
        return void cdCmd(arg);
      case "ls":
      case "dir":
      case "get-childitem":
        return void lsCmd();
      case "exit":
        return setRes("out", "alt+space hides \xB7 terminal stays running in the background");
      default: {
        if (isProfileName(cmd)) {
          if (arg) {
            const scoped = findInProfile(cmd, arg);
            if (scoped) return void launchPreset(scoped);
            return setRes("err", `no \u201C${esc(arg)}\u201D in ${esc(cmd)} \xB7 tab to browse`);
          }
          return presetRows(cmd);
        }
        const hit = findPreset(cmd);
        if (hit && !arg) return void launchPreset(hit, bg);
        if (hit && arg && hit.n === cmd) return void launchPreset(hit, bg);
        if (cmd.startsWith("retry ") && lastLaunchError) return void launchPreset(lastLaunchError.preset);
        if (cmd.startsWith("pin ") || cmd.startsWith("unpin ")) {
          const name = cmd.split(/\s+/)[1] || "";
          return void (async () => {
            ENGINE_RUNTIME = await togglePin(ENGINE_ID, name);
            setRes("ok", `&rarr; pins updated`);
            defaults();
          })();
        }
        if (!arg && !c.startsWith("!") && !c.startsWith("?") && !c.startsWith("=") && !/[\\/]/.test(c) && /^[\w.-]+$/i.test(cmd)) {
          const near = PRESETS.filter((x) => presetMatches(x, cmd)).slice(0, 6);
          if (near.length) {
            setRes("err", `no shortcut '${esc(cmd)}' \u2014 did you mean?`);
            showRows(near.map((x) => ({ c: x.n, d: `${x.cat} \xB7 ${x.d}`, cc: x.n })));
            return;
          }
          setRes("err", `no shortcut '${esc(cmd)}' \u2014 try 'shortcuts'`);
          return;
        }
        return void shellExec(c);
      }
    }
  }
  async function hideLauncherWindow(reason) {
    forceIdle(reason);
    if (mode === "terminal") {
      try {
        await detachTerminal(false);
      } catch {
        mode = "action";
        termWrap.hidden = true;
      }
    }
    inp.value = "";
    syncEcho();
    clearRes();
    applyChrome("action");
    void invoke("dismiss_launcher");
  }
  function dismissAction(reason) {
    void hideLauncherWindow(reason);
  }
  function focusPrompt() {
    if (mode === "terminal") {
      term?.focus();
      return;
    }
    try {
      inp.focus();
    } catch {
    }
  }
  function bindUi() {
    inp.addEventListener("input", () => {
      syncEcho();
      clearRes();
      refreshProposals();
    });
    inp.addEventListener("keydown", (e) => {
      if (mode !== "action") {
        return;
      }
      if (e.key === "Escape") {
        return;
      } else if (e.key === "Tab") {
        e.preventDefault();
        const r = rowSel >= 0 && rows[rowSel] || rows.find((x) => x.cc !== void 0);
        if (r) pick(r);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        if (inp.value.trim() && rows.length) {
          rowSel = Math.max(rowSel < 0 ? rows.length - 1 : rowSel - 1, 0);
          markSel();
        } else if (hist.length) {
          hi = Math.min(hi + 1, hist.length);
          inp.value = hist[hist.length - hi];
          syncEcho();
          refreshProposals();
        }
      } else if (e.key === "ArrowDown") {
        e.preventDefault();
        if (inp.value.trim() && rows.length) {
          rowSel = Math.min(rowSel + 1, rows.length - 1);
          markSel();
        } else if (hi > 0) {
          hi--;
          inp.value = hi ? hist[hist.length - hi] : "";
          syncEcho();
          refreshProposals();
        }
      } else if (e.key === "Enter") {
        e.preventDefault();
        e.stopImmediatePropagation();
        if (e.repeat || launchBlocked()) return;
        if (rowSel >= 0 && rows[rowSel]) {
          activateRow(rows[rowSel], e.ctrlKey);
          return;
        }
        submitCommand(inp.value, e.ctrlKey);
      } else if (e.ctrlKey && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "l") {
        e.preventDefault();
        inp.value = "";
        syncEcho();
        clearRes();
        defaults();
      } else if (e.ctrlKey && !e.shiftKey && e.key === "p") {
        e.preventDefault();
        if (hist.length) {
          hi = Math.min(hi + 1, hist.length);
          inp.value = hist[hist.length - hi];
          syncEcho();
          refreshProposals();
        }
      } else if (e.ctrlKey && !e.shiftKey && e.key === "n") {
        e.preventDefault();
        if (hi > 0) {
          hi--;
          inp.value = hi ? hist[hist.length - hi] : "";
          syncEcho();
          refreshProposals();
        }
      } else if (e.altKey && /^[1-9]$/.test(e.key)) {
        const pin = ENGINE_RUNTIME.pins[Number(e.key) - 1];
        if (pin) {
          e.preventDefault();
          const hit = findPreset(pin);
          if (hit) void launchPreset(hit);
        }
      } else if (e.ctrlKey && /^[1-9]$/.test(e.key)) {
        e.preventDefault();
        const idx = Number(e.key) - 1;
        if (rows[idx]) activateRow(rows[idx], e.ctrlKey);
      }
    });
    document.addEventListener(
      "keydown",
      (e) => {
        if (mode === "terminal" && (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
          e.preventDefault();
          e.stopPropagation();
          showTermFind();
          return;
        }
        const esc2 = e.key === "Escape" || e.code === "Escape";
        if (!esc2) return;
        const termOpen = mode === "terminal" || root?.dataset.mode === "terminal" || termWrap && !termWrap.hidden;
        if (termOpen) {
          if (termFindOpen) {
            e.preventDefault();
            e.stopPropagation();
            hideTermFind();
            return;
          }
          const menu = document.getElementById("cli-term-menu");
          if (menu?.classList.contains("on")) {
            e.preventDefault();
            e.stopPropagation();
            hideTermMenu();
            return;
          }
          return;
        }
        if (inp.value.trim()) {
          e.preventDefault();
          e.stopPropagation();
          if (escClearPending) {
            escClearPending = false;
            dismissAction("escape");
            return;
          }
          escClearPending = true;
          inp.value = "";
          syncEcho();
          clearRes();
          defaults();
          return;
        }
        e.preventDefault();
        e.stopPropagation();
        dismissAction("escape");
      },
      true
    );
    document.querySelector(".cli").addEventListener("click", (e) => {
      e.stopPropagation();
      clearBlurTimer();
      focusPrompt();
    });
    termWrap.addEventListener("mousedown", (e) => {
      e.stopPropagation();
      clearBlurTimer();
      if (mode === "terminal") term?.focus();
    });
    root.addEventListener("mousedown", () => clearBlurTimer());
    void getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      if (focused) {
        clearBlurTimer();
        requestAnimationFrame(focusPrompt);
        return;
      }
      if (mode === "terminal" || sessionAlive) return;
      const blurDelay = busy ? Math.max(ENGINE_OPTS.blurDismissMs, FOCUS_STEAL_GRACE_MS + 80) : ENGINE_OPTS.blurDismissMs;
      scheduleDismiss(blurDelay, "outside-blur");
    });
    window.addEventListener("resize", () => {
      if (mode === "terminal") void fitAndResizePty();
    });
  }
  async function resetBar(seed) {
    forceIdle("resetBar");
    if (mode === "terminal") {
      await detachTerminal(false);
    }
    await refreshSessionAlive();
    await refreshPresets();
    inp.value = "";
    syncEcho();
    clearRes();
    setPrompt();
    applyChrome("action");
    applySeed(seed);
    focusPrompt();
  }
  function applySeed(seed) {
    if (seed == null || seed === "") return defaults();
    const trimmed = seed.trim();
    if (trimmed === "apps" || trimmed === "start") return presetRows("apps");
    inp.value = seed;
    syncEcho();
    refreshProposals();
  }
  var _barBoot = barWindow();
  if (!_barBoot.__VERSAILLES_BAR_BOUND__) {
    _barBoot.__VERSAILLES_BAR_BOUND__ = true;
    void (async () => {
      const v = window.versailles;
      if (v?.waitForTauri) await v.waitForTauri();
      bindDom();
      bindUi();
      try {
        const ctx = await loadSpawnableEngineContext();
        ENGINE_ID = ctx.id;
        ENGINE_OPTS = ctx.opts;
        ENGINE_RUNTIME = await loadEngineRuntime(ENGINE_ID);
        hostAvailable = true;
      } catch {
        hostAvailable = typeof window.__TAURI__ !== "undefined" || !!window.versailles;
      }
      const onShown = (ev) => {
        const seed = typeof ev === "string" ? ev : typeof ev?.payload === "string" ? ev.payload : "";
        void resetBar(seed);
      };
      await listen("overlay://shown", onShown);
      await listen("launcher://shown", onShown);
      await listen("overlay://hidden", () => {
        forceIdle("hidden");
        void (async () => {
          if (mode === "terminal") await detachTerminal(false);
          inp.value = "";
          syncEcho();
          clearRes();
          applyChrome("action");
        })();
      });
      await listen("launcher://hidden", () => {
        forceIdle("hidden");
        void (async () => {
          if (mode === "terminal") await detachTerminal(false);
          inp.value = "";
          syncEcho();
          clearRes();
          applyChrome("action");
        })();
      });
      try {
        HOME = await invoke("cli_home");
      } catch {
        HOME = "";
      }
      if (!HOME) HOME = "C:\\";
      cwd = HOME;
      PRESETS = builtinPresets(HOME);
      await refreshPresets();
      await refreshSessionAlive();
      applyChrome("action");
      setPrompt();
      defaults();
      focusPrompt();
    })();
  }
})();
