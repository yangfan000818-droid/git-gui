<script lang="ts">
  interface Props {
    active?: boolean;
    intensity?: "subtle" | "medium" | "heavy";
  }

  let { active = true, intensity = "subtle" }: Props = $props();

  let canvas: HTMLCanvasElement | undefined = $state();
  let animId: number | null = null;

  const KATAKANA =
    "アイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワヲン";
  const LATIN = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
  const CHARS = KATAKANA + LATIN + "{}[]|/\\*#@$%&<>";

  const opacityMap = { subtle: 0.1, medium: 0.18, heavy: 0.28 };
  const densityMap = { subtle: 70, medium: 50, heavy: 30 };
  const speedMap = { subtle: 1, medium: 1.5, heavy: 2.2 };

  class Drop {
    x = 0;
    y = 0;
    speed = 0;
    chars: string[] = [];
    length = 0;
    color = "";

    constructor(x: number, maxY: number, speedMul: number) {
      this.x = x;
      this.y = Math.random() * maxY - maxY;
      this.speed = 0.6 + Math.random() * 2.5 * speedMul;
      this.length = 5 + Math.floor(Math.random() * 25);
      this.color = Math.random() > 0.3 ? "#00ff88" : "#00d4ff";
      this.chars = [];
      for (let i = 0; i < this.length; i++) {
        this.chars.push(CHARS[Math.floor(Math.random() * CHARS.length)]);
      }
    }

    update(h: number) {
      this.y += this.speed;
      if (this.y > h + this.length * 14) {
        this.y = -this.length * 14;
        this.speed = 0.6 + Math.random() * 2.5;
        this.chars = [];
        for (let i = 0; i < this.length; i++) {
          this.chars.push(CHARS[Math.floor(Math.random() * CHARS.length)]);
        }
      }
      // Occasionally mutate a character
      if (Math.random() < 0.02) {
        const idx = Math.floor(Math.random() * this.chars.length);
        this.chars[idx] = CHARS[Math.floor(Math.random() * CHARS.length)];
      }
    }

    draw(c: CanvasRenderingContext2D, fontSize: number) {
      for (let i = 0; i < this.chars.length; i++) {
        const charY = this.y - i * fontSize;
        if (charY < 0 || charY > c.canvas.height) continue;

        // Head of the drop is bright, tail fades
        const pos = i / this.chars.length;
        let alpha: number;
        if (i === 0) {
          alpha = 1; // Leading char is bright white
        } else if (pos < 0.1) {
          alpha = 0.7;
        } else if (pos < 0.3) {
          alpha = 0.3;
        } else {
          alpha = 0.08;
        }

        c.fillStyle =
          i === 0
            ? `rgba(200, 255, 220, ${alpha})`
            : this.color === "#00ff88"
              ? `rgba(0, 255, 136, ${alpha})`
              : `rgba(0, 212, 255, ${alpha})`;

        c.font = `${fontSize}px "JetBrains Mono", monospace`;
        c.fillText(this.chars[i], this.x, charY);
      }
    }
  }

  function render() {
    if (!canvas) return;
    const c = canvas.getContext("2d");
    if (!c) return;

    const fontSize = 13;
    const cols = Math.floor(canvas.width / densityMap[intensity]);
    const speedMul = speedMap[intensity];

    const drops: Drop[] = [];
    for (let i = 0; i < cols; i++) {
      drops.push(new Drop(i * densityMap[intensity], canvas.height, speedMul));
    }

    function frame() {
      if (!canvas || !c) return;
      c.clearRect(0, 0, canvas.width, canvas.height);
      c.globalAlpha = opacityMap[intensity];

      for (const drop of drops) {
        drop.update(canvas.height);
        drop.draw(c, fontSize);
      }

      c.globalAlpha = 1;
      animId = requestAnimationFrame(frame);
    }

    animId = requestAnimationFrame(frame);
  }

  function start() {
    if (!canvas || !active) return;
    canvas.width = window.innerWidth;
    canvas.height = window.innerHeight;
    render();
  }

  $effect(() => {
    if (active && canvas) {
      start();
    } else if (!active && animId) {
      cancelAnimationFrame(animId);
      animId = null;
      if (canvas) {
        const c = canvas.getContext("2d");
        c?.clearRect(0, 0, canvas.width, canvas.height);
      }
    }
    return () => {
      if (animId) cancelAnimationFrame(animId);
    };
  });
</script>

<canvas bind:this={canvas} class="matrix-rain-canvas" aria-hidden="true"
></canvas>

<style>
  .matrix-rain-canvas {
    position: fixed;
    inset: 0;
    pointer-events: none;
    z-index: 99995;
  }
  :global([data-animations="false"]) .matrix-rain-canvas {
    display: none;
  }
</style>
