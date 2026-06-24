<script lang="ts">
  interface Props {
    active?: boolean;
  }

  let { active = true }: Props = $props();

  let canvas: HTMLCanvasElement | undefined = $state();
  let animId: number | null = null;
  let mouseX = -100;
  let mouseY = -100;
  let mouseOnScreen = false;

  interface TrailParticle {
    x: number;
    y: number;
    life: number;
    maxLife: number;
    color: string;
    size: number;
  }
  let trail: TrailParticle[] = [];

  interface Ripple {
    x: number;
    y: number;
    radius: number;
    maxRadius: number;
    life: number;
    color: string;
  }
  let ripples: Ripple[] = [];

  const COLORS = ["#00ff88", "#00d4ff", "#ff00ff", "#ffaa00"];

  function spawnTrail(x: number, y: number) {
    trail.push({
      x,
      y,
      life: 0.6 + Math.random() * 0.4,
      maxLife: 0.6 + Math.random() * 0.4,
      color: COLORS[Math.floor(Math.random() * COLORS.length)],
      size: 1 + Math.random() * 2.5,
    });
    if (trail.length > 40) trail.shift();
  }

  function spawnRipple(x: number, y: number) {
    for (let i = 0; i < 3; i++) {
      ripples.push({
        x,
        y,
        radius: 2,
        maxRadius: 30 + i * 25,
        life: 0.5 - i * 0.1,
        color: COLORS[i % COLORS.length],
      });
    }
    if (ripples.length > 20) ripples.splice(0, 3);
  }

  function handleClick(e: MouseEvent) {
    if (!active) return;
    spawnRipple(e.clientX, e.clientY);
  }
  function handleMove(e: MouseEvent) {
    mouseX = e.clientX;
    mouseY = e.clientY;
    mouseOnScreen = true;
  }
  function handleLeave() {
    mouseOnScreen = false;
  }

  function render() {
    if (!canvas) return;
    const c = canvas.getContext("2d");
    if (!c) return;

    c.clearRect(0, 0, canvas.width, canvas.height);

    // Spawn trail particles near cursor
    if (mouseOnScreen) {
      for (let i = 0; i < 2; i++) {
        spawnTrail(
          mouseX + (Math.random() - 0.5) * 12,
          mouseY + (Math.random() - 0.5) * 12,
        );
      }
    }

    // Draw trail particles
    for (let i = trail.length - 1; i >= 0; i--) {
      const p = trail[i];
      p.life -= 0.02;
      if (p.life <= 0) {
        trail.splice(i, 1);
        continue;
      }
      const alpha = p.life / p.maxLife;
      c.beginPath();
      c.arc(p.x, p.y, p.size * alpha, 0, Math.PI * 2);
      c.fillStyle = p.color;
      c.globalAlpha = alpha * 0.5;
      c.fill();
    }

    // Draw ripples
    for (let i = ripples.length - 1; i >= 0; i--) {
      const r = ripples[i];
      r.radius += 2.5;
      r.life -= 0.02;
      if (r.life <= 0 || r.radius >= r.maxRadius) {
        ripples.splice(i, 1);
        continue;
      }
      const alpha = r.life / 0.5;
      c.beginPath();
      c.arc(r.x, r.y, r.radius, 0, Math.PI * 2);
      c.strokeStyle = r.color;
      c.lineWidth = 1.5 * alpha;
      c.globalAlpha = alpha * 0.7;
      c.stroke();
    }

    c.globalAlpha = 1;
  }

  function start() {
    if (!canvas || !active) return;
    canvas.width = window.innerWidth;
    canvas.height = window.innerHeight;

    window.addEventListener("click", handleClick);
    window.addEventListener("mousemove", handleMove);
    window.addEventListener("mouseleave", handleLeave);

    function frame() {
      if (!active) return;
      render();
      animId = requestAnimationFrame(frame);
    }
    animId = requestAnimationFrame(frame);
  }

  $effect(() => {
    if (active && canvas) start();
    return () => {
      if (animId) cancelAnimationFrame(animId);
      window.removeEventListener("click", handleClick);
      window.removeEventListener("mousemove", handleMove);
      window.removeEventListener("mouseleave", handleLeave);
    };
  });
</script>

<canvas bind:this={canvas} class="cursor-effects-canvas" aria-hidden="true"
></canvas>

<style>
  .cursor-effects-canvas {
    position: fixed;
    inset: 0;
    pointer-events: none;
    z-index: 99992;
  }
  :global([data-animations="false"]) .cursor-effects-canvas {
    display: none;
  }
</style>
