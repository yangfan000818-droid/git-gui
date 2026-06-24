<script lang="ts">
  interface Props {
    trigger: boolean;
  }

  let { trigger = false }: Props = $props();

  let canvas: HTMLCanvasElement | undefined = $state();
  let ctx: CanvasRenderingContext2D | null = $state(null);
  let animId: number | null = null;
  let particles: Particle[] = [];
  let frameCount = 0;
  const maxFrames = 90; // ~1.5s at 60fps

  const COLORS = ["#00ff88", "#ff00ff", "#00d4ff", "#ffaa00", "#ff3366"];

  class Particle {
    x = 0;
    y = 0;
    vx = 0;
    vy = 0;
    life = 1;
    decay = 0;
    color = "";
    size = 0;
    gravity = 0;

    constructor(
      x: number,
      y: number,
      angle: number,
      speed: number,
      color: string,
      size: number,
    ) {
      this.x = x;
      this.y = y;
      this.vx = Math.cos(angle) * speed;
      this.vy = Math.sin(angle) * speed;
      this.color = color;
      this.size = size;
      this.decay = 0.01 + Math.random() * 0.03;
      this.gravity = 0.03;
    }

    update() {
      this.x += this.vx;
      this.y += this.vy;
      this.vy += this.gravity;
      this.vx *= 0.99;
      this.life -= this.decay;
    }

    draw(c: CanvasRenderingContext2D) {
      if (this.life <= 0) return;
      c.save();
      c.globalAlpha = this.life;
      c.shadowColor = this.color;
      c.shadowBlur = 6;
      c.fillStyle = this.color;
      c.beginPath();
      c.arc(this.x, this.y, this.size * this.life, 0, Math.PI * 2);
      c.fill();
      c.restore();
    }
  }

  function burst(cx: number, cy: number, count: number) {
    for (let i = 0; i < count; i++) {
      const angle = (Math.PI * 2 * i) / count + (Math.random() - 0.5) * 0.3;
      const speed = 1.5 + Math.random() * 3.5;
      const color = COLORS[Math.floor(Math.random() * COLORS.length)];
      const size = 1 + Math.random() * 2;
      particles.push(new Particle(cx, cy, angle, speed, color, size));
    }
  }

  function animate() {
    if (!canvas || !ctx) return;
    const c = ctx;
    // Trail effect
    c.fillStyle = "rgba(10, 10, 15, 0.25)";
    c.fillRect(0, 0, canvas.width, canvas.height);

    for (let i = particles.length - 1; i >= 0; i--) {
      particles[i].update();
      if (particles[i].life <= 0) {
        particles.splice(i, 1);
        continue;
      }
      particles[i].draw(c);
    }
  }

  function start() {
    if (!canvas) return;
    canvas.width = window.innerWidth;
    canvas.height = window.innerHeight;
    ctx = canvas.getContext("2d");
    particles = [];
    frameCount = 0;

    // 3 bursts at different positions
    const w = canvas.width;
    const h = canvas.height;
    burst(w * 0.5, h * 0.4, 50);
    burst(w * 0.3, h * 0.35, 35);
    burst(w * 0.7, h * 0.35, 35);
    // Small delayed bursts
    setTimeout(() => {
      burst(w * 0.4, h * 0.45, 25);
      burst(w * 0.6, h * 0.45, 25);
    }, 200);

    animId = requestAnimationFrame(loop);
  }

  function loop() {
    if (frameCount >= maxFrames || particles.length === 0) {
      if (canvas && ctx) {
        ctx.clearRect(0, 0, canvas.width, canvas.height);
      }
      animId = null;
      return;
    }
    frameCount++;
    animate();
    animId = requestAnimationFrame(loop);
  }

  $effect(() => {
    if (trigger && canvas) {
      start();
    }
    return () => {
      if (animId) cancelAnimationFrame(animId);
    };
  });
</script>

<canvas bind:this={canvas} class="fireworks-canvas" aria-hidden="true"></canvas>

<style>
  .fireworks-canvas {
    position: fixed;
    inset: 0;
    pointer-events: none;
    z-index: 100000;
  }
  :global([data-animations="false"]) .fireworks-canvas {
    display: none;
  }
</style>
