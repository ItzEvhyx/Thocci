/**
 * 20-second Trivia Typing Challenge for Thocci's Home stage.
 *
 * - 20 curated trivia prompts (2 per category across 10 categories), each ~75-85 words
 *   so even ultra-fast 200+ WPM typists never run out of text during a 20s sprint.
 * - Randomized non-repeating prompt selection on every start / reset.
 * - Flow: Rules & Start -> 3-2-1 countdown -> 20s live sprint (realtime WPM & accuracy)
 *   -> locked input & fade-in Results screen with Reset and Back to Thocci controls.
 */

import { iconMarkup } from "./icons.js";

export const TYPING_DURATION_SECONDS = 20;
export const COUNTDOWN_SECONDS = 3;

export const TRIVIA_PROMPTS = [
  // 1. Tech
  {
    id: "tech-first-bug",
    category: "Tech",
    title: "The First Computer Bug",
    text: "The first real computer bug was an actual moth found trapped inside a massive Harvard relay computer in 1947. Engineers gently taped the insect directly into their daily logbook and wrote a humorous note beside it celebrating the first actual case of a bug being found. That historic notebook page still sits preserved inside the Smithsonian museum today. Since that unexpected afternoon, hunting down mysterious glitches in software has been called debugging by programmers all around the world.",
  },
  {
    id: "tech-qwerty",
    category: "Tech",
    title: "The Origin of QWERTY",
    text: "The QWERTY keyboard layout was created in the 1870s for early mechanical typewriters built with swinging metal typebars. Designers deliberately spaced common English letter pairs apart so fast typists would not jam neighboring arms together in midair. Even though modern mechanical switches and optical sensors never physically clash, billions of people still rely on that exact nineteenth century arrangement every single day because retraining the entire world proved far harder than inventing faster computers.",
  },

  // 2. Education
  {
    id: "edu-oldest-uni",
    category: "Education",
    title: "The World's Oldest University",
    text: "The oldest continuously operating university in the world was founded in Fez, Morocco in the year 859 by a visionary scholar named Fatima al-Fihri. Students traveled across deserts and seas to study mathematics, medicine, astronomy, and languages inside its quiet courtyard halls. Centuries later, its historic library still preserves rare handwritten manuscripts copied more than a thousand years ago, making it one of the longest living centers of human curiosity and shared learning on Earth.",
  },
  {
    id: "edu-spaced-repetition",
    category: "Education",
    title: "Why Spacing Beats Cramming",
    text: "Psychologists discovered back in the 1880s that short study sessions spread calmly across several days build far stronger memory than one exhausted night of cramming. Each time your brain almost forgets an idea and works slightly harder to pull it back, the underlying neural connection grows thicker and faster to fire. That simple spacing effect explains why ten focused minutes every morning effortlessly outperforms four frantic hours right before a morning exam.",
  },

  // 3. Environment
  {
    id: "env-wood-wide-web",
    category: "Environment",
    title: "How Trees Talk Underground",
    text: "Beneath almost every healthy forest floor stretches a vast invisible network of microscopic fungi linking tree roots together like living fiber optic cables. Ecologists call it the wood wide web because older mother trees share carbon, water, and chemical warnings with shaded seedlings nearby. When hungry beetles attack one branch, neighboring trees quietly receive the underground alert and start boosting bitter defensive compounds in their own leaves long before the swarm ever arrives.",
  },
  {
    id: "env-ocean-oxygen",
    category: "Environment",
    title: "Where Most Oxygen Comes From",
    text: "Most people assume tropical rainforests generate the vast majority of our breathable oxygen, yet more than half of the fresh air filling your lungs actually comes from microscopic ocean plankton. These tiny sunlit organisms drift across open currents near the surface, anchoring entire marine food webs while quietly absorbing carbon and refreshing the global atmosphere every single day. Protecting healthy coastal waters is therefore just as vital for our air as saving ancient forests.",
  },

  // 4. Health & Fitness
  {
    id: "health-sleep-rinse",
    category: "Health & Fitness",
    title: "Your Brain Runs on Sleep",
    text: "While you drift into deep sleep each night, your brain switches on a remarkable microscopic rinsing system that gently flushes out metabolic waste built up during long waking hours. Cutting rest short leaves that cellular clutter lingering behind, dulling reaction speed, weakening focus, and tilting your mood toward stress. Even a single extra hour of steady unbroken sleep measurably sharpens memory recall, athletic coordination, and calm decision making throughout the entire next day.",
  },
  {
    id: "health-short-walk",
    category: "Health & Fitness",
    title: "The Power of a Short Walk",
    text: "Taking a relaxed fifteen minute walk right after a meal helps your leg muscles absorb glucose smoothly instead of letting blood sugar spike sharply and crash an hour later. Research also reveals that steady rhythmic walking outdoors boosts creative problem solving by more than sixty percent compared with sitting still indoors. That simple daily habit explains why so many inventors, writers, and engineers discover their clearest breakthroughs the moment they step away from a desk.",
  },

  // 5. Space & Astronomy
  {
    id: "space-venus-day",
    category: "Space & Astronomy",
    title: "A Day Longer Than a Year",
    text: "Venus spins so lazily on its tilted axis that one single full rotation takes more Earth days than its entire orbital trip around the Sun. Even stranger, Venus rotates completely backward compared with almost every other planet in our solar system. If you could somehow survive its crushing atmosphere and stand beneath those thick golden clouds, you would watch the glowing Sun rise slowly in the west and drift lazily toward the eastern horizon.",
  },
  {
    id: "space-diamond-rain",
    category: "Space & Astronomy",
    title: "Diamond Rain on Ice Giants",
    text: "Thousands of kilometers inside Neptune and Uranus, staggering atmospheric pressure squeezes ordinary methane gas until individual carbon atoms snap together into sparkling solid crystals. Planetary physicists recreated those exact extreme temperatures with high powered lasers in a laboratory and confirmed that real microscopic diamonds form in fractions of a second. Over millions of years, endless storms of glittering gems likely fall steadily downward toward the superheated rocky cores of both distant worlds.",
  },

  // 6. Gaming
  {
    id: "gaming-creeper",
    category: "Gaming",
    title: "The Accidental Birth of Creeper",
    text: "One of the most recognizable creatures in video game history began purely as a silly coding mistake. While putting together a blocky pig model for an early build of Minecraft, the developer accidentally swapped the height and length dimensions in code, producing a tall awkward four legged figure. Instead of deleting the glitch, he painted it leafy green and gave it a quiet fuse, accidentally creating the iconic silently sneaking Creeper loved and feared worldwide.",
  },
  {
    id: "gaming-mario-design",
    category: "Gaming",
    title: "Why Mario Wears a Hat",
    text: "Mario earned his famous red cap, bright blue overalls, and thick dark mustache because of strict hardware limits on tiny arcade screens in 1981. Animating flowing hair or drawing a readable mouth inside a sixteen pixel grid was nearly impossible at the time, so artists used clever silhouettes instead. His hat hid static hair, his mustache separated nose from chin, and contrasting sleeves made every frantic jump immediately readable from across a crowded arcade.",
  },

  // 7. Philosophy
  {
    id: "phil-theseus",
    category: "Philosophy",
    title: "The Ship of Theseus",
    text: "Ancient Greek thinkers posed a timeless identity puzzle that still sparks lively debate in philosophy classrooms and science fiction stories today. Imagine a famous wooden ship moored in a harbor where caretakers slowly replace every weathered plank one by one until zero original timber remains. Is it still the exact same vessel it was on day one, and what happens to its true identity if someone secretly rebuilds a second ship using every discarded plank?",
  },
  {
    id: "phil-occams-razor",
    category: "Philosophy",
    title: "Occam's Razor",
    text: "A sharp fourteenth century thinker named William of Ockham popularized a practical mental tool for cutting straight through confusion. Whenever two competing explanations account for the exact same evidence equally well, the simpler explanation requiring fewer hidden assumptions is almost always the wiser place to begin testing. Although nature can certainly surprise us with subtle twists, scientists, doctors, detectives, and software engineers still reach for that clean guiding principle every single day.",
  },

  // 8. Fashion
  {
    id: "fashion-jeans-pocket",
    category: "Fashion",
    title: "Why Jeans Have That Tiny Pocket",
    text: "That small rectangular pocket stitched neatly inside the front right pocket of classic blue jeans was never originally meant for spare coins or wireless earbuds. Levi Strauss added the snug slot in the 1870s so frontier cowboys, railroad workers, and gold miners could safely tuck delicate pocket watches on metal chains inside their denim. Shielded tightly against the hip, the fragile glass face stayed safe while riders galloped across trails or swung heavy iron tools.",
  },
  {
    id: "fashion-high-heels",
    category: "Fashion",
    title: "The Surprising History of High Heels",
    text: "High heels were originally engineered for male Persian cavalry riders in the tenth century rather than glamorous modern fashion runways. The sturdy raised heel locked securely against wooden stirrups so mounted archers could rise smoothly out of the saddle and steady their aim while firing arrows at full gallop. When Persian diplomats later visited Europe, wealthy aristocrats fell in love with the bold silhouette and quickly adopted towering heels as an unmistakable symbol of power.",
  },

  // 9. Law & Government
  {
    id: "law-shortest-constitution",
    category: "Law & Government",
    title: "The World's Shortest Constitution",
    text: "Monaco governs an entire sovereign nation along the Mediterranean coast with one of the shortest active national constitutions on Earth, totaling fewer than four thousand crisp words from start to finish. By striking contrast, India maintains the longest written national constitution in world history, containing hundreds of detailed articles covering federal structure, civil rights, courts, and local administration. Together, they show how two democracies can encode law at totally different scales.",
  },
  {
    id: "law-filibuster",
    category: "Law & Government",
    title: "How the Filibuster Got Its Name",
    text: "The modern political term filibuster actually evolved from an old Dutch word describing bold sea pirates who hijacked merchant ships across the Caribbean. Lawmakers in the nineteenth century borrowed that colorful label to mock senators who effectively hijacked the legislative floor for hours on end, reading recipes or phone books aloud to stall an unpopular vote. Even today, the word captures the strange drama of winning time simply by refusing to stop talking.",
  },

  // 10. Careers & Productivity
  {
    id: "career-attention-residue",
    category: "Careers & Productivity",
    title: "The Hidden Cost of Task Switching",
    text: "Every time you step away from focused creative work just to glance at one incoming notification, a surprising slice of your working memory stays glued to the unfinished task you left behind. Cognitive researchers call this lingering drag attention residue, and studies show it can take over twenty minutes to rebuild deep momentum afterward. Guarding quiet uninterrupted blocks of single task work consistently delivers better ideas in half the effort of frantic multitasking.",
  },
  {
    id: "career-parkinsons-law",
    category: "Careers & Productivity",
    title: "Parkinson's Law",
    text: "In 1955, British historian Cyril Northcote Parkinson opened a witty essay with a sharp observation that still echoes inside every modern studio and office. Work inevitably expands to fill whatever time you casually leave available for its completion. Giving a simple task an entire lazy week invites endless second guessing, whereas setting a crisp focused time box strips away busywork, clarifies what actually matters, and often produces noticeably cleaner results in half the hours.",
  },
];

export function calculateTypingMetrics({
  correctChars,
  totalKeystrokes,
  correctKeystrokes,
  elapsedMs,
}) {
  const safeElapsedMs = Math.max(1, elapsedMs);
  const minutes = safeElapsedMs / 60000;
  const wpm =
    correctChars > 0 && safeElapsedMs >= 350
      ? Math.round(correctChars / 5 / minutes)
      : 0;
  const accuracy =
    totalKeystrokes <= 0
      ? 100
      : Math.max(0, Math.min(100, Math.round((correctKeystrokes / totalKeystrokes) * 100)));
  return { wpm, accuracy };
}

export function pickRandomPrompt(excludeId = null, rng = Math.random) {
  const pool =
    excludeId && TRIVIA_PROMPTS.length > 1
      ? TRIVIA_PROMPTS.filter((p) => p.id !== excludeId)
      : TRIVIA_PROMPTS;
  const idx = Math.floor(rng() * pool.length) % pool.length;
  return pool[idx];
}

export function mountTypingGame(container, { onBackToThocci, onStateChange }) {
  // Phases: "rules" | "countdown" | "playing" | "results"
  let phase = "rules";
  let currentPrompt = null;
  let lastPromptId = null;

  let countdownTimer = null;
  let countdownRemaining = COUNTDOWN_SECONDS;

  let sprintRaf = null;
  let sprintStartMs = 0;
  let remainingSeconds = TYPING_DURATION_SECONDS;

  // Typing cursor state
  let typedChars = []; // array of entered chars matching prompt indices 0..typedChars.length-1
  let totalKeystrokes = 0;
  let correctKeystrokes = 0;
  let charSpans = [];
  let promptViewportEl = null;
  let promptTextEl = null;
  let timerEl = null;
  let wpmEl = null;
  let accEl = null;

  function setPhase(nextPhase) {
    phase = nextPhase;
    onStateChange?.(phase);
  }

  function clearTimers() {
    if (countdownTimer) {
      clearInterval(countdownTimer);
      countdownTimer = null;
    }
    if (sprintRaf !== null) {
      cancelAnimationFrame(sprintRaf);
      sprintRaf = null;
    }
  }

  function countCorrectPositioned() {
    if (!currentPrompt) return 0;
    const target = currentPrompt.text;
    let count = 0;
    for (let i = 0; i < typedChars.length; i++) {
      if (typedChars[i] === target[i]) count++;
    }
    return count;
  }

  function updateHud(nowMs = performance.now()) {
    const elapsedMs = Math.max(0, nowMs - sprintStartMs);
    const leftSec = Math.max(0, Math.ceil((TYPING_DURATION_SECONDS * 1000 - elapsedMs) / 1000));
    remainingSeconds = leftSec;
    const { wpm, accuracy } = calculateTypingMetrics({
      correctChars: countCorrectPositioned(),
      totalKeystrokes,
      correctKeystrokes,
      elapsedMs,
    });
    if (timerEl) timerEl.textContent = `${leftSec}`;
    if (wpmEl) wpmEl.textContent = `${wpm}`;
    if (accEl) accEl.textContent = `${accuracy}`;
    return { wpm, accuracy, elapsedMs };
  }

  function syncPromptScroll() {
    if (!promptViewportEl || !promptTextEl || charSpans.length === 0) return;
    const activeIdx = Math.min(typedChars.length, charSpans.length - 1);
    const activeSpan = charSpans[activeIdx];
    if (!activeSpan) return;

    // Group distinct visual line tops so scrolling advances strictly row-by-row
    const rowTops = [];
    let lastTop = -Infinity;
    for (const span of charSpans) {
      const t = span.offsetTop;
      if (t - lastTop > 8) {
        rowTops.push(t);
        lastTop = t;
      }
    }
    const activeTop = activeSpan.offsetTop;
    let activeRow = 0;
    for (let r = 0; r < rowTops.length; r++) {
      if (Math.abs(rowTops[r] - activeTop) <= 8) {
        activeRow = r;
        break;
      }
    }
    // Keep completed rows scrolling out smoothly once the user passes row 1,
    // leaving 1 previous row, the active row in the middle, and 1 upcoming row.
    const targetRow = Math.max(0, activeRow - 1);
    const offset = rowTops[targetRow] ?? 0;
    promptTextEl.style.transform = `translate3d(0, ${-offset}px, 0)`;
  }

  function syncCaretAndChars(fromIndex = 0) {
    if (!currentPrompt) return;
    const target = currentPrompt.text;
    const len = typedChars.length;
    const start = Math.max(0, fromIndex);
    for (let i = start; i < charSpans.length; i++) {
      const span = charSpans[i];
      span.classList.remove("tg-char--correct", "tg-char--wrong", "tg-char--active");
      if (i < len) {
        if (typedChars[i] === target[i]) {
          span.classList.add("tg-char--correct");
        } else {
          span.classList.add("tg-char--wrong");
        }
      } else if (i === len) {
        span.classList.add("tg-char--active");
      }
    }
    syncPromptScroll();
  }

  function finishSprint() {
    clearTimers();
    const { wpm, accuracy } = calculateTypingMetrics({
      correctChars: countCorrectPositioned(),
      totalKeystrokes,
      correctKeystrokes,
      elapsedMs: TYPING_DURATION_SECONDS * 1000,
    });
    const wordsTyped = Math.floor(countCorrectPositioned() / 5);
    setPhase("results");
    renderResults({ wpm, accuracy, wordsTyped });
  }

  function tickSprint(nowMs) {
    if (phase !== "playing") return;
    const elapsedMs = nowMs - sprintStartMs;
    if (elapsedMs >= TYPING_DURATION_SECONDS * 1000) {
      updateHud(sprintStartMs + TYPING_DURATION_SECONDS * 1000);
      finishSprint();
      return;
    }
    updateHud(nowMs);
    sprintRaf = requestAnimationFrame(tickSprint);
  }

  function renderRules() {
    clearTimers();
    setPhase("rules");
    container.innerHTML = `
      <div class="tg-panel tg-rules">
        <div class="tg-rules-header">
          <span class="tg-badge">20s Trivia Sprint</span>
          <button type="button" class="tg-link-btn" data-action="back">${iconMarkup("chevron-right", "ui-icon--flip-x tg-back-chevron")}<span>Back to Thocci</span></button>
        </div>
        <h2 class="tg-rules-title">Ready to test your speed?</h2>
        <ul class="tg-rules-list">
          <li><strong>20 seconds on the clock:</strong> type as much of the random trivia as you can before time runs out.</li>
          <li><strong>Live WPM &amp; accuracy:</strong> calculated in real time as you type — use Backspace to fix mistakes.</li>
          <li><strong>20 randomized trivias:</strong> spans 10 topics and shuffles fresh every time you reset.</li>
        </ul>
        <div class="tg-actions">
          <button type="button" class="tg-btn-primary" data-action="start">Start challenge</button>
        </div>
      </div>
    `;
    container.querySelector('[data-action="start"]')?.addEventListener("click", startCountdown);
    container.querySelector('[data-action="back"]')?.addEventListener("click", () => {
      clearTimers();
      onBackToThocci?.();
    });
    container.querySelector('[data-action="start"]')?.focus({ preventScroll: true });
  }

  function startCountdown() {
    clearTimers();
    currentPrompt = pickRandomPrompt(lastPromptId);
    lastPromptId = currentPrompt.id;
    countdownRemaining = COUNTDOWN_SECONDS;
    setPhase("countdown");

    const drawBeat = () => {
      container.innerHTML = `
        <div class="tg-panel tg-countdown">
          <div class="tg-countdown-top">
            <span class="tg-badge">${escapeHtml(currentPrompt.category)} · ${escapeHtml(currentPrompt.title)}</span>
            <button type="button" class="tg-link-btn" data-action="back">${iconMarkup("chevron-right", "ui-icon--flip-x tg-back-chevron")}<span>Back to Thocci</span></button>
          </div>
          <div class="tg-countdown-center">
            <span class="tg-countdown-num" aria-live="assertive">${countdownRemaining}</span>
            <span class="tg-countdown-hint">Hands on the home row…</span>
          </div>
        </div>
      `;
      container.querySelector('[data-action="back"]')?.addEventListener("click", () => {
        clearTimers();
        onBackToThocci?.();
      });
    };

    drawBeat();
    countdownTimer = setInterval(() => {
      countdownRemaining -= 1;
      if (countdownRemaining <= 0) {
        clearTimers();
        beginPlaying();
      } else {
        drawBeat();
      }
    }, 1000);
  }

  function beginPlaying() {
    clearTimers();
    typedChars = [];
    totalKeystrokes = 0;
    correctKeystrokes = 0;
    remainingSeconds = TYPING_DURATION_SECONDS;
    setPhase("playing");

    container.innerHTML = `
      <div class="tg-panel tg-playing">
        <div class="tg-hud">
          <div class="tg-hud-left">
            <span class="tg-badge">${escapeHtml(currentPrompt.category)}</span>
            <span class="tg-topic-title">${escapeHtml(currentPrompt.title)}</span>
          </div>
          <div class="tg-hud-metrics" aria-live="off">
            <span class="tg-metric"><strong id="tgTimer">${TYPING_DURATION_SECONDS}</strong><small>s</small></span>
            <span class="tg-metric"><strong id="tgWpm">0</strong><small>wpm</small></span>
            <span class="tg-metric"><strong id="tgAcc">100</strong><small>% acc</small></span>
          </div>
        </div>
        <div class="tg-prompt-viewport" id="tgPromptViewport">
          <div class="tg-prompt-text" id="tgPromptText"></div>
        </div>
        <div class="tg-playing-footer">
          <button type="button" class="tg-ghost-btn" data-action="reset">↻ Restart</button>
          <button type="button" class="tg-ghost-btn" data-action="back">${iconMarkup("chevron-right", "ui-icon--flip-x tg-back-chevron")}<span>Back to Thocci</span></button>
        </div>
      </div>
    `;

    timerEl = container.querySelector("#tgTimer");
    wpmEl = container.querySelector("#tgWpm");
    accEl = container.querySelector("#tgAcc");
    promptViewportEl = container.querySelector("#tgPromptViewport");
    promptTextEl = container.querySelector("#tgPromptText");

    const frag = document.createDocumentFragment();
    charSpans = [];
    for (let i = 0; i < currentPrompt.text.length; i++) {
      const span = document.createElement("span");
      span.className = i === 0 ? "tg-char tg-char--active" : "tg-char";
      span.textContent = currentPrompt.text[i];
      frag.appendChild(span);
      charSpans.push(span);
    }
    promptTextEl.appendChild(frag);

    container.querySelector('[data-action="reset"]')?.addEventListener("click", () => {
      startCountdown();
    });
    container.querySelector('[data-action="back"]')?.addEventListener("click", () => {
      clearTimers();
      onBackToThocci?.();
    });

    // Blur buttons so Space/Enter go straight into typing
    if (document.activeElement instanceof HTMLElement) {
      document.activeElement.blur();
    }

    sprintStartMs = performance.now();
    sprintRaf = requestAnimationFrame(tickSprint);
  }

  function renderResults({ wpm, accuracy, wordsTyped }) {
    container.innerHTML = `
      <div class="tg-panel tg-results">
        <div class="tg-results-meta">
          <span class="tg-badge">${escapeHtml(currentPrompt.category)} · ${escapeHtml(currentPrompt.title)}</span>
          <span class="tg-results-sub">${wordsTyped} words in ${TYPING_DURATION_SECONDS}s</span>
        </div>
        <div class="tg-results-scores" aria-live="polite">
          <div class="tg-score-block">
            <span class="tg-score-num">${wpm}</span>
            <span class="tg-score-label">WPM</span>
          </div>
          <div class="tg-score-block">
            <span class="tg-score-num">${accuracy}<span class="tg-score-pct">%</span></span>
            <span class="tg-score-label">Accuracy</span>
          </div>
        </div>
        <div class="tg-actions">
          <button type="button" class="tg-btn-primary" data-action="reset">↻ Reset</button>
          <button type="button" class="tg-btn-secondary" data-action="back">Back to Thocci</button>
        </div>
      </div>
    `;
    container.querySelector('[data-action="reset"]')?.addEventListener("click", startCountdown);
    container.querySelector('[data-action="back"]')?.addEventListener("click", () => {
      clearTimers();
      onBackToThocci?.();
    });
    container.querySelector('[data-action="reset"]')?.focus({ preventScroll: true });
  }

  /**
   * Feed a typed character or Backspace from the keyboard controller.
   * Returns true if consumed by the active sprint.
   */
  function handleInput({ char, action }) {
    if (phase !== "playing" || !currentPrompt) return false;

    if (action === "backspace") {
      if (typedChars.length === 0) return true;
      const prevIdx = typedChars.length - 1;
      typedChars.pop();
      syncCaretAndChars(prevIdx);
      updateHud();
      return true;
    }

    if (action === "char" && typeof char === "string" && char.length === 1) {
      if (typedChars.length >= currentPrompt.text.length) {
        finishSprint();
        return true;
      }
      const idx = typedChars.length;
      const expected = currentPrompt.text[idx];
      typedChars.push(char);
      totalKeystrokes += 1;
      if (char === expected) {
        correctKeystrokes += 1;
      }
      syncCaretAndChars(idx);
      updateHud();

      if (typedChars.length >= currentPrompt.text.length) {
        finishSprint();
      }
      return true;
    }

    return false;
  }

  renderRules();

  return {
    getPhase: () => phase,
    handleInput,
    destroy() {
      clearTimers();
      container.innerHTML = "";
    },
  };
}

function escapeHtml(text) {
  return String(text ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}
