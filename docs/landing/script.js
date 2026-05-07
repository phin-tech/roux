(() => {
  const overlay = document.querySelector(".scanlines");
  const toggle = document.querySelector(".scanline-toggle");
  const STORAGE_KEY = "roux-landing.scanlines";

  if (overlay && toggle) {
    const apply = (on) => {
      overlay.dataset.scanlines = on ? "on" : "off";
      toggle.setAttribute("aria-pressed", on ? "true" : "false");
      toggle.textContent = on ? "[ CRT: ON ]" : "[ CRT: OFF ]";
    };

    let initial = false;
    try {
      initial = localStorage.getItem(STORAGE_KEY) === "1";
    } catch {
      // private mode or storage disabled — fall back to default off
    }
    apply(initial);

    toggle.addEventListener("click", () => {
      const next = overlay.dataset.scanlines !== "on";
      apply(next);
      try {
        localStorage.setItem(STORAGE_KEY, next ? "1" : "0");
      } catch {
        /* ignore */
      }
    });
  }

  // Smooth-scroll in-page anchor links
  document.querySelectorAll('a[href^="#"]').forEach((a) => {
    a.addEventListener("click", (e) => {
      const id = a.getAttribute("href").slice(1);
      if (!id) return;
      const target = document.getElementById(id);
      if (!target) return;
      e.preventDefault();
      target.scrollIntoView({ behavior: "smooth", block: "start" });
      history.replaceState(null, "", `#${id}`);
    });
  });
})();
