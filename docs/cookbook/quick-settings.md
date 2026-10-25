# Quick settings

A slide-out at the top right, opened with **Ctrl+Alt+Q**, that does what Windows' Quick Settings and Action Center do: Wi-Fi on / off and networks, Bluetooth and audio devices, volume, the microphone and the output, a per-app mixer, brightness and night light, Do Not Disturb, recent notifications, Windows VPN profiles, the mobile hotspot and accessibility toggles. Changes made in Windows show up here too: every value arrives by push.

## Hooks

| Hook | Class | Used for |
| ---- | ----- | -------- |
| `hotkey` | (declaration) | Ctrl+Alt+Q toggles the panel |
| `radios` | control | Wi-Fi, Bluetooth and airplane mode on / off |
| `wifi` | control | Networks in range; scan, join, disconnect |
| `bluetooth` | control | Paired devices; connect / disconnect headphones and speakers |
| `audio` | plain | Volume and mute of the default output and microphone (read) |
| `audio-control` | control | Set volume and mute, the default output and microphone, each app's volume |
| `display` | control | Brightness per screen, night light, projection (Win+P) |
| `dnd` | control | Do Not Disturb: off, priority only, alarms only |
| `notify` | plain | Recent notifications (read) |
| `notify-control` | control | Dismiss one, clear all, open the app that raised one |
| `vpn` | control | Windows' own VPN profiles; connect / disconnect |
| `hotspot` | control | Mobile hotspot on / off and its client count |
| `accessibility` | control | Magnifier, contrast theme, live captions, colour filters, sticky keys |

Control class: reading works anywhere, but a change works only from a bar, slide-out or overlay (never the wallpaper) and only inside a click or key press. Each change answers the hook's new state; a refusal rejects with a message that says why. More hooks for the same panel: `power-mode`, `sharing` (Cast, nearby sharing), see [HOOKS.md › Quick settings](../HOOKS.md#quick-settings).

## Snippet

```html
<template class="spawnable" data-id="qs" data-anchor="tr" data-w="340" data-h="720"
          data-hotkey="Ctrl+Alt+Q"
          data-hooks="hotkey,radios,wifi,bluetooth,audio,audio-control,display,dnd,notify,notify-control,vpn,hotspot,accessibility">
  <style>
    .qs { height: 100%; box-sizing: border-box; overflow: auto; padding: 14px; display: flex; flex-direction: column; gap: 8px;
          background: var(--paper, #f9fafb); color: var(--ink, #141414); font: 13px/1.3 var(--font-sans, "Segoe UI", sans-serif); }
    .qs h3 { margin: 8px 0 0; font-size: 11px; letter-spacing: 0.08em; text-transform: uppercase; color: var(--ink-mut, #6b7280); }
    .qs .row { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
    .qs button { padding: 5px 9px; border: 1px dashed currentColor; border-radius: 8px; background: none; color: inherit; font: inherit; cursor: pointer; }
    .qs button.on { background: var(--ink, #141414); color: var(--paper, #f9fafb); border-style: solid; }
    .qs input[type=range], .qs select { flex: 1; min-width: 0; }
    .qs .err { color: #b91c1c; min-height: 1em; }
  </style>
  <div class="qs" data-no-drag>
    <h3>wi-fi</h3><div class="row"><button data-radio="wifi">wi-fi</button><button class="scan">scan</button></div><div class="row nets"></div>
    <form class="row join" hidden><input type="password" placeholder="password"><button>join</button></form>
    <h3>bluetooth</h3><div class="row"><button data-radio="bluetooth">bluetooth</button></div><div class="row devs"></div>
    <h3>sound</h3>
    <div class="row"><input class="vol" type="range" min="0" max="100"><button class="mute">mute</button><button class="mic">mic</button></div>
    <div class="row"><select class="out"></select></div>
    <div class="mixer"></div>
    <h3>display</h3><div class="row"><input class="bright" type="range" min="0" max="100"><button class="night">night light</button></div>
    <h3>do not disturb</h3><div class="row dnd"><button data-mode="off">off</button><button data-mode="priority">priority</button><button data-mode="alarms">alarms</button></div>
    <h3>notifications <button class="clear">clear all</button></h3><div class="toasts"></div>
    <h3>vpn</h3><div class="row vpns"></div>
    <h3>hotspot</h3><div class="row"><button class="hotspot">hotspot</button></div>
    <h3>accessibility</h3>
    <div class="row a11y"><button data-a11y="magnifier">magnifier</button><button data-a11y="captions">captions</button><button data-a11y="stickyKeys">sticky keys</button></div>
    <div class="err"></div>
  </div>
  <script>
    (function quickSettings() {
      const root = document.currentScript.closest('.spawn-surface');
      const $ = (s) => root.querySelector(s);
      const $$ = (s) => [...root.querySelectorAll(s)];
      const button = (text, on, onclick) => Object.assign(document.createElement('button'), { type: 'button', textContent: text, className: on ? 'on' : '', onclick });

      (function whenSdk(fn) {
        typeof window.versailles?.for === 'function' ? fn(window.versailles) : setTimeout(() => whenSdk(fn), 50);
      })((v) => {
        const me = v.for('qs');
        // A change rejects with a reason when Windows refuses: show it.
        const act = (p) => { $('.err').textContent = ''; return p.catch((e) => { $('.err').textContent = e.message; }); };

        // Radios
        let radios = {};
        me.radios.onChange((r) => {
          radios = r;
          $$('[data-radio]').forEach((b) => { b.classList.toggle('on', r[b.dataset.radio] === true); b.disabled = r[b.dataset.radio] == null; });
        });
        $$('[data-radio]').forEach((b) => (b.onclick = () => act(me.radios.set(b.dataset.radio, !radios[b.dataset.radio]))));

        // Wi-Fi: Windows refreshes the list only on a scan, so scan when the panel opens.
        const join = $('.join');
        me.wifi.onChange((w) => {
          $('.nets').replaceChildren(...(w.access !== 'ok' ? [] : w.networks.slice(0, 6).map((n) =>
            button(n.ssid + ' ' + n.signal + '%', n.connected, () => {
              if (n.connected) return act(me.wifi.disconnect());
              if (n.secure && !n.known) { join.dataset.ssid = n.ssid; join.hidden = false; return join.querySelector('input').focus(); }
              act(me.wifi.connect(n.ssid));
            }))));
          if (w.access === 'location-denied') $('.nets').textContent = 'Turn on location access to list networks.';
        });
        join.onsubmit = (e) => {   // Enter or the button: a key press or click, as a change needs
          e.preventDefault();
          join.hidden = true;
          act(me.wifi.connect(join.dataset.ssid, join.querySelector('input').value));
          join.querySelector('input').value = '';
        };
        const scan = () => me.wifi.scan().catch(() => {});
        $('.scan').onclick = scan;
        me.on('shown', scan);

        // Bluetooth: paired devices; only audio devices can be connected from here.
        me.bluetooth.onChange((b) => {
          $('.devs').replaceChildren(...b.devices.filter((d) => d.audio).map((d) =>
            button(d.name, d.connected, () => act(d.connected ? me.bluetooth.disconnect(d.id) : me.bluetooth.connect(d.id)))));
        });

        // Volume and microphone are read through `audio`, set through `audio-control`.
        let audio = {};
        me.audio.onChange((a) => {
          audio = a;
          if (document.activeElement !== $('.vol')) $('.vol').value = a.volume ?? 0;
          $('.mute').classList.toggle('on', !!a.muted);
          $('.mic').disabled = !a.input;
          $('.mic').classList.toggle('on', !!a.input?.muted);
        });
        $('.vol').oninput = (e) => act(me.audioControl.setVolume(Number(e.target.value)));
        $('.mute').onclick = () => act(me.audioControl.setMute(!audio.muted));
        $('.mic').onclick = () => act(me.audioControl.setInputMute(!audio.input?.muted));

        // Outputs and the per-app mixer.
        me.audioControl.onChange((c) => {
          if (document.activeElement !== $('.out')) {
            $('.out').replaceChildren(...c.outputs.map((o) => Object.assign(document.createElement('option'), { value: o.id, textContent: o.name, selected: o.default })));
          }
          if (!$('.mixer').contains(document.activeElement)) {
            $('.mixer').replaceChildren(...c.sessions.map((s) => {
              const row = Object.assign(document.createElement('label'), { className: 'row', textContent: s.app + ' ' });
              const range = Object.assign(document.createElement('input'), { type: 'range', min: 0, max: 100, value: s.volume });
              range.oninput = () => act(me.audioControl.setSessionVolume(s.id, Number(range.value)));
              row.append(range);
              return row;
            }));
          }
        });
        $('.out').onchange = (e) => act(me.audioControl.setDefaultOutput(e.target.value));

        // Display: the first screen's brightness, night light.
        let display = null;
        me.display.onChange((d) => {
          display = d;
          const screen = d.brightness[0];
          $('.bright').disabled = !screen;
          if (screen && document.activeElement !== $('.bright')) $('.bright').value = screen.level;
          $('.night').disabled = !d.nightLight.supported;
          $('.night').classList.toggle('on', d.nightLight.on === true);
        });
        $('.bright').onchange = (e) => act(me.display.setBrightness(display.brightness[0].id, Number(e.target.value)));
        $('.night').onclick = () => act(me.display.setNightLight(!display.nightLight.on));

        // Do Not Disturb
        me.dnd.onChange((d) => $$('.dnd button').forEach((b) => { b.classList.toggle('on', b.dataset.mode === d.mode); b.disabled = !d.supported; }));
        $$('.dnd button').forEach((b) => (b.onclick = () => act(me.dnd.set(b.dataset.mode))));

        // Notifications: `notify` pushes new ones only; dismissing answers the new list.
        const drawToasts = (n) => {
          if (n.access !== 'allowed') { $('.toasts').textContent = 'Notification access is off (Settings > Privacy & security > Notifications).'; return; }
          $('.toasts').replaceChildren(...n.items.slice(0, 5).map((t) =>
            button((t.app ? t.app + ': ' : '') + t.title + ' ×', false, () => act(me.notifyControl.dismiss(t.id)).then((r) => r && drawToasts(r)))));
        };
        const readToasts = () => me.notify.now().then(drawToasts, () => {});
        me.notify.onChange(readToasts);
        me.on('shown', readToasts);
        readToasts();
        $('.clear').onclick = () => act(me.notifyControl.clear()).then((r) => r && drawToasts(r));

        // VPN: without a saved sign-in, Windows' own dialog asks ({ ok: true, dialog: true }).
        me.vpn.onChange((s) => {
          $('.vpns').replaceChildren(...s.profiles.map((p) =>
            button(p.connecting ? p.name + '…' : p.name, p.connected, () => act(p.connected ? me.vpn.disconnect(p.name) : me.vpn.connect(p.name)))));
        });

        // Hotspot: shares the connection the PC is online with.
        let hotspot = null;
        me.hotspot.onChange((h) => {
          hotspot = h;
          $('.hotspot').classList.toggle('on', h.on === true);
          $('.hotspot').disabled = !h.supported || h.busy;
          $('.hotspot').textContent = h.on ? 'hotspot · ' + (h.clients ?? 0) + ' connected' : 'hotspot';
        });
        $('.hotspot').onclick = () => act(me.hotspot.set(!hotspot.on));

        // Accessibility: null = not installed or unknown.
        let a11y = {};
        me.accessibility.onChange((a) => {
          a11y = a;
          $$('[data-a11y]').forEach((b) => { b.classList.toggle('on', a[b.dataset.a11y] === true); b.disabled = a[b.dataset.a11y] == null; });
        });
        $$('[data-a11y]').forEach((b) => (b.onclick = () => act(me.accessibility.set(b.dataset.a11y, !a11y[b.dataset.a11y]))));
      });
    })();
  </script>
</template>
```

## Gotchas

- **`data-no-drag`.** A slide-out (`data-anchor="tr"`) can be dragged from any plain element, which swallows clicks on rows. `data-no-drag` on the panel keeps every click a click.
- **Clicks only.** A change outside a click or key press rejects with `needs-gesture`; from the wallpaper with `hook-refused`. Reading (`now()`, `onChange`) works anywhere. `wifi.scan()` needs no click.
- **Nothing to change.** Many values are `null` or `supported: false` where Windows offers no switch: `airplane` behind a hardware slider, night light Versailles cannot re-encode, power mode without the Balanced plan, Do Not Disturb where Windows does not offer it, a PC without a hotspot (`reason` says why). Disable the control, or offer the Settings page instead: `me.shell.open('ms-settings:nightlight')` (that needs `shell` in `data-hooks`).
- **Wi-Fi** may need location access on Windows 11 24H2 (`access: "location-denied"`). Joining a new secured network takes its password once; Windows saves a profile. Enterprise sign-in is not supported. Ask for the password inside the panel (as here), not with `prompt()`: a click counts for a few seconds only, so after a slow dialog the change may be refused (`needs-gesture`).
- **Bluetooth** lists paired devices only. Headphones and speakers connect through their audio driver; mice and keyboards reconnect by themselves and only show their state.
- **Volume** is read from `audio` and set with `audio-control`: declare both. `setVolume`, `setInputVolume`, `setSessionVolume` take 0–100.
- **The mixer is private:** it shows which apps play sound. Declare `audio-control` only on the piece that needs it.
- **Brightness** of external monitors goes over DDC/CI: slow, and not every monitor answers. A screen that answers neither way is missing from `brightness`.
- **Notifications** need Windows' notification access for Versailles (`access: "allowed"`). New ones are pushed; ones removed in Windows are not, so read `notify.now()` again when the panel opens. A notification's own click action cannot be run from outside: `notifyControl.open(aumid)` starts its app.
- **Answers that are not a state.** `vpn.connect` and `accessibility.set("contrast", false)` may open Windows' own dialog or Settings page and answer `{ ok: true, dialog: true }` or `{ ok: true, settings: true }`; the new state follows by push.
- **A contrast theme** turns on CSS `forced-colors: active` in every page, this panel included. Style for it with `@media (forced-colors: active)` if you care.

The starter's complete panel (with inline Wi-Fi passwords, power mode, projection, microphone and output pickers, Cast and nearby sharing) is commented out near the end of the page: `try me (quick settings)`.
