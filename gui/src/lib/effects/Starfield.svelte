<script lang="ts">
  interface Props {
    active?: boolean;
    density?: number;
    speed?: number;
  }

  let { active = true, density = 120, speed = 0.15 }: Props = $props();

  let canvas: HTMLCanvasElement | undefined = $state();
  let animId: number | null = null;

  interface Star {
    x: number;
    y: number;
    r: number;
    baseAlpha: number;
    alpha: number;
    twinkleSpeed: number;
    twinklePhase: number;
    color: string;
  }

  let stars: Star[] = [];
  const COLORS = [
    "#ffffff",
    "#ffffff",
    "#ffffff",
    "#ffffff",
    "#ffffff",
    "#00ff88",
    "#00d4ff",
    "#ffaa00",
    "#ff00ff",
    "#ffffff",
  ];

  function init(w: number, h: number) {
    stars = [];
    for (let i = 0; i < density; i++) {
      const baseAlpha = 0.2 + Math.random() * 0.6;
      stars.push({
        x: Math.random() * w,
        y: Math.random() * h,
        r: 0.3 + Math.random() * 1.4,
        baseAlpha,
        alpha: baseAlpha,
        twinkleSpeed: 0.005 + Math.random() * 0.03,
        twinklePhase: Math.random() * Math.PI * 2,
        color: COLORS[Math.floor(Math.random() * COLORS.length)],
      });
    }
  }

  function render(time: number) {
    if (!canvas) return;
    const c = canvas.getContext("2d");
    if (!c) return;

    c.clearRect(0, 0, canvas.width, canvas.height);

    for (const star of stars) {
      // Twinkle
      star.alpha =
        star.baseAlpha +
        Math.sin(time * star.twinkleSpeed + star.twinklePhase) * 0.25;
      star.alpha = Math.max(0.05, Math.min(0.9, star.alpha));

      // Slow drift
      star.x += (Math.sin(time * 0.0003 + star.y * 0.01) * speed) / 60;
      star.y += (Math.cos(time * 0.0004 + star.x * 0.01) * speed) / 60;

      // Wrap around
      if (star.x < -5) star.x = canvas.width + 5;
      if (star.x > canvas.width + 5) star.x = -5;
      if (star.y < -5) star.y = canvas.height + 5;
      if (star.y > canvas.height + 5) star.y = -5;

      c.beginPath();
      c.arc(star.x, star.y, star.r, 0, Math.PI * 2);
      c.fillStyle = star.color;
      c.globalAlpha = star.alpha * 0.5;
      c.fill();

      // Bright stars get a glow
      if (star.r > 1 && star.alpha > 0.5) {
        c.beginPath();
        c.arc(star.x, star.y, star.r * 2.5, 0, Math.PI * 2);
        c.fillStyle = star.color;
        c.globalAlpha = star.alpha * 0.1;
        c.fill();
      }
    }
    c.globalAlpha = 1;
  }

  function start() {
    if (!canvas || !active) return;
    canvas.width = window.innerWidth;
    canvas.height = window.innerHeight;
    init(canvas.width, canvas.height);

    function frame(time: number) {
      if (!active) return;
      render(time);
      animId = requestAnimationFrame(frame);
    }
    animId = requestAnimationFrame(frame);
  }

  $effect(() => {
    if (active && canvas) {
      start();
    } else if (!active && animId) {
      cancelAnimationFrame(animId);
      animId = null;
      const c = canvas?.getContext("2d");
      if (c && canvas) c.clearRect(0, 0, canvas.width, canvas.height);
    }
    return () => {
      if (animId) cancelAnimationFrame(animId);
    };
  });
</script>

<canvas bind:this={canvas} class="starfield-canvas" aria-hidden="true"></canvas>

<style>
  .starfield-canvas {
    position: fixed;
    inset: 0;
    pointer-events: none;
    z-index: 99990;
  }
  :global([data-animations="false"]) .starfield-canvas {
    display: none;
  }
</style>
