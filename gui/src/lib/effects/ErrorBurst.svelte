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
  const maxFrames = 60; // ~1s at 60fps

  const COLORS = ["#ff3366", "#ff0066", "#ff6600", "#ff00ff", "#ff1133"];

  class Particle {
    x = 0;
    y = 0;
    vx = 0;
    vy = 0;
    life = 1;
    decay = 0;
    color = "";
    size = 0;

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
      this.decay = 0.025 + Math.random() * 0.04;
      // Error particles fall faster
      this.vy -= 1.5;
    }

    update() {
      this.x += this.vx;
      this.y += this.vy;
      this.vy += 0.06; // stronger gravity
      this.vx *= 0.98;
      this.life -= this.decay;
    }

    draw(c: CanvasRenderingContext2D) {
      if (this.life <= 0) return;
      c.save();
      c.globalAlpha = this.life;
      c.shadowColor = this.color;
      c.shadowBlur = 4;
      // Square particles for "spark" feel
      c.fillStyle = this.color;
      const s = this.size * this.life;
      c.fillRect(this.x - s / 2, this.y - s / 2, s, s);
      c.restore();
    }
  }

  function burst(cx: number, cy: number, count: number) {
    for (let i = 0; i < count; i++) {
      const angle = (Math.PI * 2 * i) / count + (Math.random() - 0.5) * 0.5;
      const speed = 2 + Math.random() * 4;
      const color = COLORS[Math.floor(Math.random() * COLORS.length)];
      const size = 1.5 + Math.random() * 2.5;
      particles.push(new Particle(cx, cy, angle, speed, color, size));
    }
  }

  function animate() {
    if (!canvas || !ctx) return;
    const c = ctx;
    c.fillStyle = "rgba(10, 10, 15, 0.3)";
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

    const w = canvas.width;
    const h = canvas.height;
    // Burst from near-top (where error banner appears)
    burst(w * 0.5, 48, 30);
    burst(w * 0.4, 52, 20);
    burst(w * 0.6, 50, 20);
    // Small delayed spark
    setTimeout(() => burst(w * 0.5, 60, 15), 150);

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

<canvas bind:this={canvas} class="error-burst-canvas" aria-hidden="true"
></canvas>

<style>
  .error-burst-canvas {
    position: fixed;
    inset: 0;
    pointer-events: none;
    z-index: 100001;
  }
  :global([data-animations="false"]) .error-burst-canvas {
    display: none;
  }
</style>
