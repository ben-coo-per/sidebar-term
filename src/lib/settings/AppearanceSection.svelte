<!-- The Settings page's Appearance section: the app's fonts and colours and the Terminal's, each
     with its default to go back to. A change shows at once, here and on every Terminal.
     State: src/lib/appearance/appearance.svelte.ts; what can be chosen: src/lib/appearance/model.ts. -->
<script lang="ts">
  import {
    appearance,
    fontInstalled,
    resetColors,
    resetFonts,
    setColor,
    setFontFamily,
    setFontNumber,
    setTerminalColor,
    uiDefaults,
  } from "../appearance/appearance.svelte";
  import {
    ANSI_COLORS,
    FONT_DEFAULTS,
    FONT_RANGES,
    MONO_FONT_SUGGESTIONS,
    TERMINAL_COLOR_GROUPS,
    UI_COLOR_GROUPS,
    UI_FONT_SUGGESTIONS,
    fontFamilies,
    isCustom,
    isKeywordFamily,
    parseColor,
    terminalColor,
    terminalLook,
    type ColorDef,
    type FontFamily,
    type FontNumber,
  } from "../appearance/model";

  const FAMILIES: { key: FontFamily; label: string; detail: string; placeholder: string; suggestions: string }[] = [
    {
      key: "ui",
      label: "App font",
      detail: "The sidebar, the window bar, Manager and Settings.",
      placeholder: "System font",
      suggestions: "appearance-ui-fonts",
    },
    {
      key: "mono",
      label: "App fixed-width font",
      detail: "Paths, commands and codes outside the Terminal.",
      placeholder: "SF Mono",
      suggestions: "appearance-mono-fonts",
    },
  ];
  const TERMINAL_FAMILY = {
    key: "terminal",
    label: "Terminal font",
    detail: "Every Terminal. A fixed-width font keeps columns in line.",
    placeholder: "SF Mono",
    suggestions: "appearance-mono-fonts",
  } as const;

  const WEIGHT_NAMES: Record<number, string> = {
    100: "Thin",
    200: "Extra light",
    300: "Light",
    400: "Regular",
    500: "Medium",
    600: "Semibold",
    700: "Bold",
    800: "Extra bold",
    900: "Black",
  };

  function steps(key: FontNumber): number[] {
    const { min, max, step } = FONT_RANGES[key];
    return Array.from({ length: Math.round((max - min) / step) + 1 }, (_, i) => Number((min + i * step).toFixed(2)));
  }

  const fonts = $derived({ ...FONT_DEFAULTS, ...appearance.fonts });
  const look = $derived(terminalLook(appearance));
  const custom = $derived(isCustom(appearance));

  /** The families chosen for a font that this Mac has no font for. */
  function missing(key: FontFamily): string[] {
    return fontFamilies(fonts[key]).filter((name) => !isKeywordFamily(name) && !fontInstalled(name));
  }

  /** What was typed for a colour and is none, said under its row. */
  let refused = $state<{ id: string; text: string } | null>(null);

  function typedColor(id: string, input: HTMLInputElement, current: string, set: (color: string) => void) {
    const text = input.value.trim();
    const color = parseColor(text);
    refused = color || !text ? null : { id, text };
    if (color) set(color);
    input.value = color ?? current;
  }

  function typedFamily(key: FontFamily, input: HTMLInputElement) {
    setFontFamily(key, input.value);
    input.value = appearance.fonts[key] ?? "";
  }

  function typedNumber(key: FontNumber, input: HTMLInputElement) {
    setFontNumber(key, input.value === "" ? null : input.valueAsNumber);
    input.value = String(fonts[key]);
  }

  function restoreDefaults() {
    refused = null;
    resetColors();
    resetFonts();
  }
</script>

{#snippet familyRow(f: { key: FontFamily; label: string; detail: string; placeholder: string; suggestions: string })}
  {@const absent = missing(f.key)}
  <li class="row">
    <span class="label">
      {f.label}
      <span class="detail">{f.detail}</span>
      {#if absent.length}
        <span class="detail warn">Not installed on this Mac: {absent.join(", ")}.</span>
      {/if}
    </span>
    <span class="controls">
      {#if appearance.fonts[f.key] !== undefined}
        <button type="button" class="text-btn small" onclick={() => setFontFamily(f.key, "")}>Reset</button>
      {/if}
      <input
        class="field family"
        type="text"
        spellcheck="false"
        autocomplete="off"
        autocapitalize="off"
        list={f.suggestions}
        placeholder={f.placeholder}
        value={appearance.fonts[f.key] ?? ""}
        onchange={(e) => typedFamily(f.key, e.currentTarget)}
        aria-label={f.label}
      />
    </span>
  </li>
{/snippet}

{#snippet numberReset(key: FontNumber)}
  {#if appearance.fonts[key] !== undefined}
    <button type="button" class="text-btn small" onclick={() => setFontNumber(key, null)}>Reset</button>
  {/if}
{/snippet}

{#snippet colorRow(def: ColorDef, color: string, isDefault: boolean, set: (color: string | null) => void)}
  <li class="row">
    <span class="label">
      {def.label}
      {#if def.detail}
        <span class="detail">{def.detail}</span>
      {/if}
      {#if refused?.id === def.id}
        <span class="detail error">“{refused.text}” is no colour. Six hex digits, as in #5b93f5.</span>
      {/if}
    </span>
    <span class="controls">
      {#if !isDefault}
        <button
          type="button"
          class="text-btn small"
          onclick={() => {
            refused = null;
            set(null);
          }}
        >
          Reset
        </button>
      {/if}
      <input
        class="field hex"
        type="text"
        spellcheck="false"
        autocomplete="off"
        autocapitalize="off"
        maxlength="9"
        value={color}
        onchange={(e) => typedColor(def.id, e.currentTarget, color, set)}
        aria-label="{def.label}, as hex"
      />
      <input
        class="swatch"
        type="color"
        value={color}
        oninput={(e) => {
          refused = null;
          set(e.currentTarget.value);
        }}
        aria-label={def.label}
      />
    </span>
  </li>
{/snippet}

<section>
  <div class="section-header">
    <div>
      <h2>Appearance</h2>
      <p class="hint">The app's fonts and colours, and the Terminal's. A change shows at once.</p>
    </div>
    <button type="button" class="text-btn" onclick={restoreDefaults} disabled={!custom}>Restore Defaults</button>
  </div>

  <datalist id="appearance-ui-fonts">
    {#each UI_FONT_SUGGESTIONS as name (name)}
      <option value={name}></option>
    {/each}
  </datalist>
  <datalist id="appearance-mono-fonts">
    {#each MONO_FONT_SUGGESTIONS as name (name)}
      <option value={name}></option>
    {/each}
  </datalist>

  <h3>Fonts</h3>
  <ul class="rows">
    {#each FAMILIES as f (f.key)}
      {@render familyRow(f)}
    {/each}
    <li class="row">
      <span class="label">
        App text size
        <span class="detail">Of the default size. Rows grow with it.</span>
      </span>
      <span class="controls">
        {@render numberReset("uiScale")}
        <select
          class="select"
          value={fonts.uiScale}
          onchange={(e) => setFontNumber("uiScale", Number(e.currentTarget.value))}
          aria-label="App text size"
        >
          {#each steps("uiScale") as scale (scale)}
            <option value={scale}>{Math.round(scale * 100)}%</option>
          {/each}
        </select>
      </span>
    </li>
  </ul>

  <h3>Terminal font</h3>
  <ul class="rows">
    {@render familyRow(TERMINAL_FAMILY)}
    <li class="row">
      <span class="label">
        Size
        <span class="detail">In pixels, {FONT_RANGES.terminalSize.min} to {FONT_RANGES.terminalSize.max}.</span>
      </span>
      <span class="controls">
        {@render numberReset("terminalSize")}
        <input
          class="field number"
          type="number"
          {...FONT_RANGES.terminalSize}
          value={fonts.terminalSize}
          onchange={(e) => typedNumber("terminalSize", e.currentTarget)}
          aria-label="Terminal font size"
        />
      </span>
    </li>
    <li class="row">
      <span class="label">
        Line height
        <span class="detail">
          Of the font's own, {FONT_RANGES.terminalLineHeight.min} to {FONT_RANGES.terminalLineHeight.max}.
        </span>
      </span>
      <span class="controls">
        {@render numberReset("terminalLineHeight")}
        <input
          class="field number"
          type="number"
          {...FONT_RANGES.terminalLineHeight}
          value={fonts.terminalLineHeight}
          onchange={(e) => typedNumber("terminalLineHeight", e.currentTarget)}
          aria-label="Terminal line height"
        />
      </span>
    </li>
    <li class="row">
      <span class="label">
        Weight
        <span class="detail">Bold text stays bold. A font shows the weights it comes in.</span>
      </span>
      <span class="controls">
        {@render numberReset("terminalWeight")}
        <select
          class="select"
          value={fonts.terminalWeight}
          onchange={(e) => setFontNumber("terminalWeight", Number(e.currentTarget.value))}
          aria-label="Terminal font weight"
        >
          {#each steps("terminalWeight") as weight (weight)}
            <option value={weight}>{WEIGHT_NAMES[weight]}</option>
          {/each}
        </select>
      </span>
    </li>
  </ul>

  <!-- What a Terminal looks like under the choices: the page covers the real ones. -->
  <div
    class="preview"
    aria-hidden="true"
    style:background={look.theme.background}
    style:color={look.theme.foreground}
    style:font-family={look.fontFamily}
    style:font-size="{look.fontSize}px"
    style:font-weight={look.fontWeight}
    style:line-height={1.2 * look.lineHeight}
  >
    <div>
      <span style:color={look.theme.green}>you@mac</span>
      <span style:color={look.theme.blue}>~/repos/sidebar-term</span>
      <span style:color={look.theme.magenta}>main</span>
      $ git status <span style:color={look.theme.brightBlack}># dim text</span>
    </div>
    <div>
      <span style:color={look.theme.red}>deleted:</span> old.ts
      <span style:color={look.theme.green}>new file:</span> new.ts
      <span style:color={look.theme.yellow}>warning:</span> <b>bold</b>
      <span style:color={look.theme.cyan}>src/lib</span>
    </div>
    <div>
      <span style:background={look.theme.selectionBackground}>selected text</span>
      0O 1lI {"{}"} =&gt; != <span class="cursor" style:background={look.theme.cursor} style:color={look.theme.cursorAccent}>c</span>
    </div>
    <div class="palette">
      {#each ANSI_COLORS as c (c.id)}
        <span style:background={terminalColor(appearance, c.id)} title={c.label}></span>
      {/each}
    </div>
  </div>

  {#each TERMINAL_COLOR_GROUPS as group (group.title)}
    <h3>{group.title}</h3>
    <ul class="rows">
      {#each group.colors as def (def.id)}
        {@render colorRow(def, terminalColor(appearance, def.id), appearance.terminal[def.id] === undefined, (color) =>
          setTerminalColor(def.id, color),
        )}
      {/each}
    </ul>
  {/each}

  {#each UI_COLOR_GROUPS as group (group.title)}
    <h3>{group.title}</h3>
    <ul class="rows">
      {#each group.colors as def (def.id)}
        {@render colorRow(
          def,
          appearance.colors[def.id] ?? uiDefaults.colors[def.id] ?? "#000000",
          appearance.colors[def.id] === undefined,
          (color) => setColor(def.id, color),
        )}
      {/each}
    </ul>
  {/each}
</section>

<style>
  .family {
    width: 200px;
  }
  .number {
    width: 64px;
    font-variant-numeric: tabular-nums;
  }
  .hex {
    width: calc(74px * var(--ui-font-scale));
    padding: 0 6px;
    font-family: var(--font-mono);
    font-size: calc(11px * var(--ui-font-scale));
  }
  /* The colour itself: a click opens the system's colour picker. */
  .swatch {
    appearance: none;
    flex: none;
    width: 30px;
    height: var(--control-height);
    padding: 0;
    border: 1px solid var(--sidebar-border);
    border-radius: var(--radius-sm);
    background: none;
    overflow: hidden;
    cursor: pointer;
  }
  .swatch::-webkit-color-swatch-wrapper {
    padding: 0;
  }
  .swatch::-webkit-color-swatch {
    border: none;
  }
  .swatch:focus-visible {
    outline: none;
    box-shadow: 0 0 0 2px var(--focus-ring);
  }
  .preview {
    margin-top: 12px;
    padding: 10px 12px;
    border: 1px solid var(--sidebar-border);
    border-radius: var(--radius-md);
    overflow: hidden;
    word-break: break-word;
  }
  .preview b {
    font-weight: bold;
  }
  .palette {
    display: grid;
    grid-template-columns: repeat(8, 1fr);
    gap: 3px;
    margin-top: 8px;
    line-height: 0;
  }
  .palette span {
    height: 12px;
    border-radius: 2px;
  }
  @container settings-pane (max-width: 380px) {
    .family {
      width: 150px;
    }
  }
</style>
