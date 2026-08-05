<script lang="ts">
  type View = 'home' | 'hr' | 'ecg' | 'hrv'
  let { onopen }: { onopen: (v: View) => void } = $props()

  const tools: { view: View; title: string; desc: string }[] = [
    {
      view: 'hr',
      title: 'HR COMPARE',
      desc: 'Side-by-side heart-rate and RR capture across sources.',
    },
    {
      view: 'ecg',
      title: 'RAW ECG',
      desc: 'Clinical-scope 130 Hz ECG with beat detection and accelerometer.',
    },
    {
      view: 'hrv',
      title: 'RHYTHM / HRV',
      desc: 'Live rhythm strip, tachogram and RMSSD.',
    },
  ]
</script>

<main class="landing">
  <section class="hero">
    <h1>ELDURO <span class="heart">&hearts;</span></h1>
    <p class="tag">POLAR H10 SIGNAL LAB</p>
    <p class="lede">
      Research-grade capture of ECG, accelerometer and heart-rate variability
      from the Polar H10, recorded losslessly and viewed live.
    </p>
  </section>

  <section class="cards">
    {#each tools as t (t.view)}
      <button class="card" onclick={() => onopen(t.view)}>
        <span class="card-title">{t.title}</span>
        <span class="card-desc">{t.desc}</span>
        <span class="card-go">Open &rarr;</span>
      </button>
    {/each}
  </section>
</main>

<style>
  .landing {
    height: calc(100vh - 58px);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 40px;
    padding: 40px 20px;
    box-sizing: border-box;
  }
  .hero {
    text-align: center;
    max-width: 640px;
  }
  .hero h1 {
    font-family: var(--font-display);
    font-size: 56px;
    letter-spacing: 4px;
    margin: 0;
  }
  .hero .heart {
    color: var(--color-heart);
  }
  .hero .tag {
    font-family: var(--font-display);
    font-size: 16px;
    letter-spacing: 3px;
    color: var(--color-slate);
    margin: 6px 0 18px;
  }
  .hero .lede {
    font-size: 16px;
    line-height: 1.5;
    color: var(--color-slate);
    margin: 0;
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: 16px;
    width: 100%;
    max-width: 900px;
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    text-align: left;
    padding: 20px;
    border: 1px solid var(--color-line);
    border-radius: 12px;
    background: var(--color-card);
    cursor: pointer;
    transition:
      border-color 0.15s,
      transform 0.15s;
  }
  .card:hover {
    border-color: var(--color-accent);
    transform: translateY(-2px);
  }
  .card-title {
    font-family: var(--font-display);
    font-size: 18px;
    letter-spacing: 1.5px;
  }
  .card-desc {
    font-size: 14px;
    line-height: 1.4;
    color: var(--color-slate);
    flex: 1;
  }
  .card-go {
    font-size: 13px;
    font-weight: 600;
    color: var(--color-accent);
  }
</style>
