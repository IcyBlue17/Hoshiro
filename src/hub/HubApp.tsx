import { StatisticsPage } from "./Statistics";
import { useCallback, useEffect, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import "./hub.css";
import { api, listen } from "../lib/ipc";
import { EVENTS, type Appearance } from "../lib/types";
import { FeedbackProvider } from "./feedback";
import { AppContext, type Page, type SettingsCategory, useSettingsStore } from "./store";
import { Sidebar } from "./Sidebar";
import { Home } from "./Home";
import { HistoryPage } from "./History";
import { DictionaryPage } from "./Dictionary";
import { SettingsModal } from "./settings/SettingsModal";
import { Onboarding } from "./Onboarding";
import { UpdateProvider } from "./update";

function useTheme(appearance: Appearance | undefined) {
  useEffect(() => {
    const root = document.documentElement;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const resolved = appearance === "light" ? "light" : appearance === "dark" ? "dark" : mq.matches ? "dark" : "light";
      root.setAttribute("data-theme", resolved);
      root.style.colorScheme = resolved;
    };
    apply();
    if (appearance === "system" || !appearance) {
      mq.addEventListener("change", apply);
      return () => mq.removeEventListener("change", apply);
    }
  }, [appearance]);
}

// Dev-only: `?onboarding=1` previews the onboarding flow in the browser.
const forceOnboarding =
  import.meta.env.DEV && typeof location !== "undefined" && new URLSearchParams(location.search).get("onboarding") === "1";

export default function HubApp() {
  const { settings, update, reload } = useSettingsStore();
  const [page, setPage] = useState<Page>("home");
  const [settingsOpen, setSettingsOpen] = useState<{ category?: SettingsCategory } | null>(null);
  const [version, setVersion] = useState("");
  const [onboardingForced, setOnboardingForced] = useState(forceOnboarding);

  useTheme(settings?.appearance);

  useEffect(() => {
    api.appInfo().then((i) => setVersion(i.version));
  }, []);

  const openSettings = useCallback((category?: SettingsCategory) => setSettingsOpen({ category }), []);
  const navigate = useCallback((p: Page) => setPage(p), []);

  const handleRoute = useCallback(
    (p: string) => {
      if (p === "settings") openSettings();
      else if (p === "settings:about") openSettings("about");
      else if (p === "home" || p === "history" || p === "dictionary") setPage(p);
    },
    [openSettings],
  );

  // The hub window is created on demand: subscribe first, then pull a route requested during load.
  useEffect(() => {
    let un: (() => void) | undefined;
    let alive = true;
    listen<string>(EVENTS.navigate, handleRoute).then((u) => {
      if (!alive) return u();
      un = u;
      api.takePendingRoute().then((r) => r && alive && handleRoute(r));
    });
    return () => {
      alive = false;
      un?.();
    };
  }, [handleRoute]);

  if (!settings) return <div className="hub-loading" />;

  const showOnboarding = settings.onboardingCompleted === false || onboardingForced;

  return (
    <FeedbackProvider>
      <AppContext.Provider value={{ settings, update, openSettings, navigate, version }}>
        <UpdateProvider>
        {showOnboarding ? (
          <Onboarding
            onDone={async () => {
              await api.completeOnboarding();
              setOnboardingForced(false);
              await reload();
            }}
          />
        ) : (
          <div className="hub">
            <div className="drag-strip" data-tauri-drag-region />
            <Sidebar page={page} />
            <main className="main">
              <AnimatePresence mode="wait" initial={false}>
                <motion.div
                  key={page}
                  className="page"
                  initial={{ opacity: 0, y: 6 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0 }}
                  transition={{ duration: 0.16, ease: "easeOut" }}
                >
                  {page === "home" && <Home />}
                  {page === "history" && <HistoryPage />}
                  {page === "dictionary" && <DictionaryPage />}
                  {page === "statistics" && <StatisticsPage />}
                </motion.div>
              </AnimatePresence>
            </main>
            <SettingsModal
              open={!!settingsOpen}
              initialCategory={settingsOpen?.category}
              onClose={() => setSettingsOpen(null)}
            />
          </div>
        )}
        </UpdateProvider>
      </AppContext.Provider>
    </FeedbackProvider>
  );
}
