<script lang="ts">
  interface Props {
    active?: boolean;
    count?: number;
    maxDist?: number;
  }

  let { active = true, count = 55, maxDist = 150 }: Props = $props();

  let canvas: HTMLCanvasElement | undefined = $state();
  let animId: number | null = null;

  interface Dot {
    x: number;
    y: number;
    vx: number;
    vy: number;
    r: number;
    color: string;
  }

  let dots: Dot[] = [];
  const COLORS = ["#00ff88", "#00d4ff", "#ff00ff", "#ffaa00"];

  function init(w: number, h: number) {
    dots = [];
    for (let i = 0; i < count; i++) {
      dots.push({
        x: Math.random() * w,
        y: Math.random() * h,
        vx: (Math.random() - 0.5) * 0.35,
        vy: (Math.random() - 0.5) * 0.35,
        r: 1.2 + Math.random() * 2,
        color: COLORS[Math.floor(Math.random() * COLORS.length)],
      });
    }
  }

  function render() {
    if (!canvas) return;
    const c = canvas.getContext("2d");
    if (!c) return;
    const w = canvas.width;
    const h = canvas.height;

    c.clearRect(0, 0, w, h);

    // Move dots
    for (const d of dots) {
      d.x += d.vx;
      d.y += d.vy;
      if (d.x < -10) d.x = w + 10;
      if (d.x > w + 10) d.x = -10;
      if (d.y < -10) d.y = h + 10;
      if (d.y > h + 10) d.y = -10;
    }

    // Draw connections
    for (let i = 0; i < dots.length; i++) {
      for (let j = i + 1; j < dots.length; j++) {
        const dx = dots[i].x - dots[j].x;
        const dy = dots[i].y - dots[j].y;
        const dist = Math.sqrt(dx * dx + dy * dy);
        if (dist < maxDist) {
          const alpha = (1 - dist / maxDist) * 0.15;
          c.strokeStyle = `rgba(0, 255, 136, ${alpha})`;
          c.lineWidth = 0.4;
          c.beginPath();
          c.moveTo(dots[i].x, dots[i].y);
          c.lineTo(dots[j].x, dots[j].y);
          c.stroke();
        }
      }
    }

    // Draw dots
    for (const d of dots) {
      c.beginPath();
      c.arc(d.x, d.y, d.r, 0, Math.PI * 2);
      c.fillStyle = d.color;
      c.globalAlpha = 0.45;
      c.fill();

      // Glow halo for dots
      c.beginPath();
      c.arc(d.x, d.y, d.r * 3, 0, Math.PI * 2);
      c.fillStyle = d.color;
      c.globalAlpha = 0.08;
      c.fill();
    }
    c.globalAlpha = 1;
  }

  function start() {
    if (!canvas || !active) return;
    canvas.width = window.innerWidth;
    canvas.height = window.innerHeight;
    init(canvas.width, canvas.height);

    function frame() {
      if (!active) return;
      render();
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

<canvas bind:this={canvas} class="particle-net-canvas" aria-hidden="true"
></canvas>

<style>
  .particle-net-canvas {
    position: fixed;
    inset: 0;
    pointer-events: none;
    z-index: 99991;
  }
  :global([data-animations="false"]) .particle-net-canvas {
    display: none;
  }
</style>
