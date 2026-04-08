document.querySelectorAll(".search-form").forEach((form) => {
  const input = form.querySelector('input[name="term"]');
  const clear = form.querySelector(".clear");
  const list = form.querySelector(".suggestions");
  if (!input || !clear || !list) return;

  // Clear button
  function toggleClear() {
    if (input.value) {
      clear.removeAttribute("hidden");
    } else {
      clear.setAttribute("hidden", "");
    }
  }
  toggleClear();
  input.addEventListener("input", toggleClear);
  clear.addEventListener("click", () => {
    input.value = "";
    toggleClear();
    hideSuggestions();
    input.focus();
  });

  // Autocomplete
  let debounceTimer = null;
  let controller = null;
  let activeIndex = -1;

  function showSuggestions(results) {
    if (results.length === 0) {
      hideSuggestions();
      return;
    }
    list.innerHTML = "";
    results.forEach((text, i) => {
      const li = document.createElement("li");
      li.textContent = text;
      li.setAttribute("role", "option");
      li.id = "suggest-" + i;
      li.addEventListener("mousedown", (e) => {
        e.preventDefault();
        navigate(text);
      });
      list.appendChild(li);
    });
    activeIndex = -1;
    list.removeAttribute("hidden");
    input.setAttribute("aria-expanded", "true");
  }

  function hideSuggestions() {
    list.setAttribute("hidden", "");
    list.innerHTML = "";
    activeIndex = -1;
    input.setAttribute("aria-expanded", "false");
    input.removeAttribute("aria-activedescendant");
  }

  function setActive(index) {
    const items = list.children;
    for (const li of items) li.removeAttribute("data-active");
    if (index >= 0 && index < items.length) {
      items[index].setAttribute("data-active", "");
      input.setAttribute("aria-activedescendant", items[index].id);
    } else {
      input.removeAttribute("aria-activedescendant");
    }
    activeIndex = index;
  }

  function navigate(term) {
    window.location.href = "/translate/" + encodeURIComponent(term);
  }

  input.addEventListener("input", () => {
    clearTimeout(debounceTimer);
    if (controller) controller.abort();
    const q = input.value.trim();
    if (!q) {
      hideSuggestions();
      return;
    }
    debounceTimer = setTimeout(() => {
      controller = new AbortController();
      fetch("/api/suggest?q=" + encodeURIComponent(q), {
        signal: controller.signal,
      })
        .then((r) => r.json())
        .then((data) => showSuggestions(data.results || []))
        .catch(() => { });
    }, 200);
  });

  input.addEventListener("keydown", (e) => {
    const items = list.children;
    if (!items.length) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive(activeIndex < items.length - 1 ? activeIndex + 1 : 0);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive(activeIndex > 0 ? activeIndex - 1 : items.length - 1);
    } else if (e.key === "Enter" && activeIndex >= 0) {
      e.preventDefault();
      navigate(items[activeIndex].textContent);
    } else if (e.key === "Escape") {
      hideSuggestions();
    }
  });

  input.addEventListener("blur", hideSuggestions);
});
