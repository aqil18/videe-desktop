import { useEffect, useState, type ReactNode } from "react";
import {
  premiereExtensionStatus,
  premiereInstallExtension,
  premierePing,
  resolveInstallScript,
  resolvePing,
  resolveScriptStatus,
} from "../lib/api";

export type Nle = "resolve" | "premiere";

interface NleConfig {
  title: string;
  installLabel: string;
  reinstallLabel: string;
  getInstalledPath: () => Promise<string | null>;
  install: () => Promise<string>;
  ping: () => Promise<boolean>;
  instructions: ReactNode;
}

const NLES: Record<Nle, NleConfig> = {
  resolve: {
    title: "DaVinci Resolve",
    installLabel: "Install Script",
    reinstallLabel: "Reinstall Script",
    getInstalledPath: resolveScriptStatus,
    install: resolveInstallScript,
    ping: resolvePing,
    instructions: (
      <>
        In DaVinci Resolve: Workspace → Scripts → Utility → Videe to start the bridge, then use "Send to DaVinci"
        from the library grid.
      </>
    ),
  },
  premiere: {
    title: "Premiere Pro",
    installLabel: "Install Extension",
    reinstallLabel: "Reinstall Extension",
    getInstalledPath: premiereExtensionStatus,
    install: premiereInstallExtension,
    ping: premierePing,
    instructions: (
      <>
        Restart Premiere Pro after installing, then open Window → Extensions → Videe and dock the panel (it reopens
        with your workspace). Then use "Send to Premiere" from the library grid. Installing also enables unsigned CEP
        extensions (PlayerDebugMode), which this panel needs.
      </>
    ),
  },
};

interface NleBridgePanelProps {
  nle: Nle;
  onClose: () => void;
}

type ConnectionStatus = "unknown" | "checking" | "connected" | "not-connected";

export function NleBridgePanel({ nle, onClose }: NleBridgePanelProps) {
  const config = NLES[nle];
  const [installedPath, setInstalledPath] = useState<string | null>(null);
  const [isInstalling, setIsInstalling] = useState(false);
  const [installError, setInstallError] = useState<string | null>(null);
  const [connectionStatus, setConnectionStatus] = useState<ConnectionStatus>("unknown");

  useEffect(() => {
    config.getInstalledPath().then(setInstalledPath);
  }, [config]);

  async function handleInstall() {
    setIsInstalling(true);
    setInstallError(null);
    try {
      const path = await config.install();
      setInstalledPath(path);
    } catch (e) {
      setInstallError(String(e));
    } finally {
      setIsInstalling(false);
    }
  }

  async function handleTestConnection() {
    setConnectionStatus("checking");
    const ok = await config.ping();
    setConnectionStatus(ok ? "connected" : "not-connected");
  }

  return (
    <div className="absolute right-4 top-14 z-20 w-96 rounded-lg border border-neutral-800 bg-neutral-900 p-4 shadow-xl">
      <div className="mb-3 flex items-center justify-between">
        <h2 className="text-sm font-medium text-neutral-100">{config.title}</h2>
        <button onClick={onClose} className="text-neutral-500 transition hover:text-neutral-200" aria-label="Close">
          ×
        </button>
      </div>

      <div className="flex flex-col gap-3 text-xs">
        <div>
          <p className="text-neutral-400">
            {installedPath ? (
              <>
                Installed at <span className="break-all text-neutral-300">{installedPath}</span>
              </>
            ) : (
              "Not installed"
            )}
          </p>
          <button
            onClick={handleInstall}
            disabled={isInstalling}
            className="mt-2 rounded-md bg-neutral-800 px-3 py-1.5 text-neutral-100 transition hover:bg-neutral-700 disabled:opacity-50"
          >
            {isInstalling ? "Installing…" : installedPath ? config.reinstallLabel : config.installLabel}
          </button>
          {installError && <p className="mt-2 text-red-400">{installError}</p>}
        </div>

        <div className="flex items-center gap-2 border-t border-neutral-800 pt-3">
          <StatusDot status={connectionStatus} />
          <button onClick={handleTestConnection} className="text-neutral-300 transition hover:text-neutral-100">
            Test Connection
          </button>
          <StatusLabel status={connectionStatus} />
        </div>

        <p className="border-t border-neutral-800 pt-3 text-neutral-500">{config.instructions}</p>
      </div>
    </div>
  );
}

function StatusDot({ status }: { status: ConnectionStatus }) {
  const color =
    status === "connected"
      ? "bg-emerald-500"
      : status === "not-connected"
        ? "bg-red-500"
        : status === "checking"
          ? "bg-yellow-500"
          : "bg-neutral-600";
  return <span className={`h-2 w-2 rounded-full ${color}`} />;
}

function StatusLabel({ status }: { status: ConnectionStatus }) {
  switch (status) {
    case "connected":
      return <span className="text-emerald-500">Connected</span>;
    case "not-connected":
      return <span className="text-red-400">Not running</span>;
    case "checking":
      return <span className="text-neutral-500">Checking…</span>;
    default:
      return null;
  }
}
