<script lang="ts">
  import { onMount } from "svelte";

  interface Props {
    ondone?: () => void;
  }

  let { ondone }: Props = $props();

  let lines = $state<string[]>([]);
  let done = $state(false);
  let fading = $state(false);

  const BOOT_SEQUENCE = [
    { text: "Initializing kernel subsystem...", delay: 80 },
    { text: "Loading git-core engine v0.11.1...", delay: 120 },
    { text: "Establishing secure channel (gRPC)...", delay: 100 },
    { text: "Mounting repository filesystem...", delay: 90 },
    { text: "Calibrating neural network interface...", delay: 150 },
    { text: "Loading theme: cyberpunk // #00ff88...", delay: 100 },
    { text: "Starting GUI render pipeline...", delay: 80 },
    { text: "Connecting to local git daemon...", delay: 130 },
    { text: "System initialized.", delay: 60 },
  ];

  let cursorVisible = $state(true);

  onMount(() => {
    // Cursor blink
    const blinkInterval = setInterval(() => {
      cursorVisible = !cursorVisible;
    }, 500);

    let i = 0;
    function typeNext() {
      if (i >= BOOT_SEQUENCE.length) {
        // Done typing — hold for 400ms then fade
        setTimeout(() => {
          fading = true;
          setTimeout(() => {
            done = true;
            clearInterval(blinkInterval);
            ondone?.();
          }, 500);
        }, 400);
        return;
      }
      const item = BOOT_SEQUENCE[i];
      let charIdx = 0;
      const interval = setInterval(() => {
        charIdx++;
        lines = [
          ...BOOT_SEQUENCE.slice(0, i).map((l) => l.text),
          item.text.slice(0, charIdx),
        ];
        if (charIdx >= item.text.length) {
          clearInterval(interval);
          i++;
          // Small gap between lines
          setTimeout(typeNext, item.delay);
        }
      }, 15);
    }

    // Initial 200ms pause then start
    setTimeout(typeNext, 200);

    return () => clearInterval(blinkInterval);
  });
</script>

{#if !done}
  <div class="boot-overlay" class:fade-out={fading} aria-hidden="true">
    <div class="boot-screen">
      <div class="boot-header">
        <span class="boot-dot dot-r"></span>
        <span class="boot-dot dot-y"></span>
        <span class="boot-dot dot-g"></span>
        <span class="boot-label">git-gui terminal v2.0</span>
      </div>
      <div class="boot-body">
        {#each lines as line}
          <div class="boot-line">
            <span class="boot-prefix">&gt;</span>
            <span class="boot-text">{line}</span>
            <span class="boot-ok">[OK]</span>
          </div>
        {/each}
        <div class="boot-line boot-cursor-line">
          <span class="boot-prefix">&gt;</span>
          <span class="boot-cursor">{cursorVisible ? "|" : " "}</span>
        </div>
      </div>
    </div>
  </div>
{/if}

<style>
  .boot-overlay {
    position: fixed;
    inset: 0;
    z-index: 200000;
    background: #06080d;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: opacity 0.5s ease;
  }
  .boot-overlay.fade-out {
    opacity: 0;
    pointer-events: none;
  }
  .boot-screen {
    font-family: var(--font-mono, "JetBrains Mono", monospace);
    font-size: 13px;
    min-width: 500px;
    background: #0a0a12;
    border: 1px solid #1a1a2e;
    border-radius: 6px;
    overflow: hidden;
    box-shadow:
      0 0 40px rgba(0, 255, 136, 0.15),
      0 16px 60px rgba(0, 0, 0, 0.8);
  }
  .boot-header {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 14px;
    background: #0e0e1a;
    border-bottom: 1px solid #1a1a2e;
  }
  .boot-dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
  }
  .dot-r {
    background: #ff5f57;
  }
  .dot-y {
    background: #febc2e;
  }
  .dot-g {
    background: #28c840;
  }
  .boot-label {
    flex: 1;
    text-align: center;
    font-size: 11px;
    color: #555;
    letter-spacing: 0.1em;
  }
  .boot-body {
    padding: 16px 20px;
    min-height: 200px;
  }
  .boot-line {
    display: flex;
    gap: 8px;
    align-items: baseline;
    margin-bottom: 6px;
    line-height: 1.5;
  }
  .boot-prefix {
    color: #00ff88;
    text-shadow: 0 0 6px rgba(0, 255, 136, 0.4);
    flex-shrink: 0;
  }
  .boot-text {
    color: #8ecfb0;
    flex: 1;
  }
  .boot-ok {
    color: #00ff88;
    font-weight: 700;
    text-shadow: 0 0 4px rgba(0, 255, 136, 0.3);
  }
  .boot-cursor {
    color: #00ff88;
    text-shadow: 0 0 8px rgba(0, 255, 136, 0.6);
  }
  .boot-cursor-line {
    margin-bottom: 0;
  }
</style>
