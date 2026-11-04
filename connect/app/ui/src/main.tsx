import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import React from "react";
import ReactDOM from "react-dom/client";
import "./style.css";

type ObsConfig = {
	install_kind: "native" | "flatpak" | "custom_binary";
	binary_path: string | null;
	config_dir_override: string | null;
	shared_config: boolean;
	websocket_port: number | null;
	websocket_password_override: string | null;
	profile: string | null;
	collection: string | null;
	scene: string | null;
	last_obs_version: string | null;
	minimize_to_tray: boolean;
	disable_updater: boolean;
	start_obs_with_connect: boolean;
	use_dashboard_obs_settings: boolean;
};
type Setup = {
	id: string;
	name: string;
	paired: boolean;
	switcher_id: string | null;
	obs_state: string;
	obs: ObsConfig;
	detected_binary: string;
	config_dir: string;
};
type State = {
	start_at_login: boolean;
	api_url: string;
	startup_error: string | null;
	first_run: boolean;
	setups: Setup[];
};
type Pairing = {
	device_code: string;
	user_code: string;
	expires_in: number;
	interval: number;
};

function App() {
	const [state, setState] = React.useState<State | null>(null);
	const [name, setName] = React.useState("");
	const [error, setError] = React.useState("");
	const [pairing, setPairing] = React.useState<Pairing | null>(null);
	const [pairingSetupId, setPairingSetupId] = React.useState<string | null>(
		null,
	);
	const [pairingStatus, setPairingStatus] = React.useState("");
	const [apiUrl, setApiUrl] = React.useState("");
	const [firstRunStartAtLogin, setFirstRunStartAtLogin] = React.useState(true);
	const [editing, setEditing] = React.useState<Setup | null>(null);
	const [secondsLeft, setSecondsLeft] = React.useState(0);
	React.useEffect(() => {
		invoke<State>("get_state")
			.then((value) => {
				setState(value);
				setApiUrl(value.api_url);
				if (value.startup_error) setError(value.startup_error);
			})
			.catch((e) => setError(String(e)));
		let unlistenCreated: (() => void) | undefined;
		let unlistenStatus: (() => void) | undefined;
		let unlistenState: (() => void) | undefined;
		let unlistenError: (() => void) | undefined;
		void listen<Pairing>("pairing-created", (event) =>
			setPairing(event.payload),
		).then((fn) => {
			unlistenCreated = fn;
		});
		void listen<string>("pairing-status", (event) =>
			setPairingStatus(event.payload),
		).then((fn) => {
			unlistenStatus = fn;
		});
		void listen<State>("connect-state-changed", (event) => {
			setState(event.payload);
			setApiUrl(event.payload.api_url);
			setPairing(null);
			setPairingSetupId(null);
		}).then((fn) => {
			unlistenState = fn;
		});
		void listen<string>("connect-error", (event) =>
			setError(event.payload),
		).then((fn) => {
			unlistenError = fn;
		});
		return () => {
			unlistenCreated?.();
			unlistenStatus?.();
			unlistenState?.();
			unlistenError?.();
		};
	}, []);
	React.useEffect(() => {
		if (!pairing) return;
		setSecondsLeft(pairing.expires_in);
		const countdown = window.setInterval(
			() => setSecondsLeft((seconds) => Math.max(0, seconds - 1)),
			1000,
		);
		return () => window.clearInterval(countdown);
	}, [pairing]);
	async function addSetup() {
		try {
			if (state?.first_run)
				await invoke("save_settings", {
					startAtLogin: firstRunStartAtLogin,
					apiUrl,
				});
			setState(await invoke<State>("add_setup", { name }));
			setName("");
		} catch (e) {
			setError(String(e));
		}
	}
	async function saveSetup() {
		if (!editing) return;
		try {
			setState(
				await invoke<State>("save_setup", {
					setupId: editing.id,
					name: editing.name,
					obs: editing.obs,
				}),
			);
			setEditing(null);
		} catch (e) {
			setError(String(e));
		}
	}
	function updateObs<K extends keyof ObsConfig>(key: K, value: ObsConfig[K]) {
		setEditing((current) =>
			current ? { ...current, obs: { ...current.obs, [key]: value } } : current,
		);
	}
	async function startPairing(id: string) {
		setError("");
		setPairing(null);
		setPairingSetupId(id);
		setPairingStatus("Waiting for confirmation…");
		try {
			await invoke("pair_setup", { setupId: id });
		} catch (e) {
			setPairingSetupId(null);
			setError(String(e));
			setPairingStatus("");
		}
	}
	async function obsAction(
		action: "start_obs" | "stop_obs" | "restart_obs",
		id: string,
	) {
		try {
			setState(await invoke<State>(action, { setupId: id }));
		} catch (e) {
			setError(String(e));
		}
	}
	async function saveSettings(startAtLogin = state?.start_at_login ?? true) {
		try {
			setState(await invoke<State>("save_settings", { startAtLogin, apiUrl }));
		} catch (e) {
			setError(String(e));
		}
	}
	async function unpair(id: string) {
		try {
			setState(await invoke<State>("unpair_setup", { setupId: id }));
		} catch (e) {
			setError(String(e));
		}
	}
	async function enableWebsocket(id: string) {
		try {
			setState(await invoke<State>("enable_websocket", { setupId: id }));
		} catch (e) {
			setError(String(e));
		}
	}
	if (!state)
		return (
			<main>
				<h1>NOALBS Cloud Connect</h1>
				<p>{error || "Loading…"}</p>
			</main>
		);
	return (
		<main>
			<header>
				<div>
					<p className="eyebrow">NOALBS CLOUD</p>
					<h1>Connect</h1>
					<p>Another way to connect this PC alongside the browser source.</p>
				</div>
			</header>
			{state.first_run && (
				<section>
					<h2>Welcome to Connect</h2>
					<label>
						<input
							type="checkbox"
							checked={firstRunStartAtLogin}
							onChange={(event) =>
								setFirstRunStartAtLogin(event.target.checked)
							}
						/>
						Start NOALBS Cloud Connect when my computer starts
					</label>
					<p className="note">
						Connect is another way to connect a PC alongside the browser source.
					</p>
				</section>
			)}
			<section>
				<h2>Your setups</h2>
				{state.setups.map((setup) => (
					<article key={setup.id}>
						<div>
							<strong>{setup.name}</strong>
							<p>
								{setup.paired
									? `Connected to switcher ${setup.switcher_id}`
									: "Not paired"}{" "}
								· OBS{" "}
								{setup.obs_state === "unknown"
									? "running, not identified"
									: setup.obs_state.replaceAll("_", " ")}
							</p>
						</div>
						<div className="actions">
							<button
								type="button"
								onClick={() => obsAction("start_obs", setup.id)}
							>
								Start
							</button>
							<button
								type="button"
								onClick={() => obsAction("stop_obs", setup.id)}
							>
								Stop
							</button>
							<button
								type="button"
								onClick={() => obsAction("restart_obs", setup.id)}
							>
								Restart
							</button>
							<button
								type="button"
								className="quiet"
								onClick={() => setEditing(structuredClone(setup))}
							>
								Edit setup
							</button>
							<button type="button" onClick={() => startPairing(setup.id)}>
								{setup.paired ? "Pair again" : "Pair"}
							</button>
							{setup.paired && (
								<button
									type="button"
									className="quiet"
									onClick={() => unpair(setup.id)}
								>
									Unpair
								</button>
							)}
							{setup.obs_state === "websocket_disabled" && (
								<button
									type="button"
									className="quiet"
									onClick={() => enableWebsocket(setup.id)}
								>
									Enable OBS websocket
								</button>
							)}
						</div>
					</article>
				))}
				<div className="add">
					<input
						aria-label="Setup name"
						placeholder="Setup name"
						value={name}
						onChange={(e) => setName(e.target.value)}
					/>
					<button type="button" onClick={addSetup}>
						Add setup
					</button>
				</div>
			</section>
			{editing && (
				<section>
					<h2>Edit setup</h2>
					<label>
						Setup name
						<input
							value={editing.name}
							onChange={(event) =>
								setEditing({ ...editing, name: event.target.value })
							}
						/>
					</label>
					<label>
						OBS install
						<select
							value={editing.obs.install_kind}
							onChange={(event) =>
								updateObs(
									"install_kind",
									event.target.value as ObsConfig["install_kind"],
								)
							}
						>
							<option value="native">Native</option>
							<option value="flatpak">Flatpak</option>
							<option value="custom_binary">Custom binary</option>
						</select>
					</label>
					<label>
						Detected OBS binary
						<input
							value={editing.obs.binary_path ?? editing.detected_binary}
							onChange={(event) =>
								updateObs("binary_path", event.target.value.trim() || null)
							}
						/>
					</label>
					<label>
						OBS config directory override
						<input
							value={editing.obs.config_dir_override ?? editing.config_dir}
							onChange={(event) =>
								updateObs(
									"config_dir_override",
									event.target.value.trim() || null,
								)
							}
						/>
					</label>
					<label>
						<input
							type="checkbox"
							checked={editing.obs.shared_config}
							onChange={(event) =>
								updateObs("shared_config", event.target.checked)
							}
						/>
						Use shared OBS config
					</label>
					{editing.obs.shared_config && (
						<>
							<p className="note">
								Shared-config mode uses the same OBS profile and websocket
								server as another OBS instance. Be sure the port and password
								match that server.
							</p>
							<label>
								Websocket port
								<input
									type="number"
									value={editing.obs.websocket_port ?? ""}
									onChange={(event) =>
										updateObs(
											"websocket_port",
											event.target.value ? Number(event.target.value) : null,
										)
									}
								/>
							</label>
							<label>
								Websocket password
								<input
									type="password"
									value={editing.obs.websocket_password_override ?? ""}
									onChange={(event) =>
										updateObs(
											"websocket_password_override",
											event.target.value || null,
										)
									}
								/>
							</label>
						</>
					)}
					{(
						[
							["profile", "Profile"],
							["collection", "Scene collection"],
							["scene", "Scene"],
						] as const
					).map(([key, label]) => (
						<label key={key}>
							{label}
							<input
								value={editing.obs[key] ?? ""}
								onChange={(event) => updateObs(key, event.target.value || null)}
							/>
						</label>
					))}
					{(
						[
							["minimize_to_tray", "Minimize OBS to tray"],
							["disable_updater", "Disable OBS updater"],
							["start_obs_with_connect", "Start OBS with Connect"],
							[
								"use_dashboard_obs_settings",
								"Use dashboard OBS websocket settings",
							],
						] as const
					).map(([key, label]) => (
						<label key={key}>
							<input
								type="checkbox"
								checked={editing.obs[key]}
								onChange={(event) => updateObs(key, event.target.checked)}
							/>
							{label}
						</label>
					))}
					<div className="actions">
						<button type="button" onClick={saveSetup}>
							Save setup
						</button>
						<button
							type="button"
							className="quiet"
							onClick={() => setEditing(null)}
						>
							Cancel
						</button>
					</div>
				</section>
			)}
			{pairing && (
				<section className="pairing">
					<h2>Pair this computer</h2>
					<p>Enter this code on the NOALBS Cloud pairing page:</p>
					<strong className="code">{pairing.user_code}</strong>
					<button
						type="button"
						onClick={() =>
							pairingSetupId &&
							invoke("open_verification_url", {
								setupId: pairingSetupId,
							}).catch((e) => setError(String(e)))
						}
					>
						Open browser
					</button>
					<p>
						{pairingStatus || "Waiting for approval…"} · Expires in{" "}
						{Math.floor(secondsLeft / 60)}:
						{String(secondsLeft % 60).padStart(2, "0")}
					</p>
				</section>
			)}
			<section>
				<h2>Settings</h2>
				<label>
					<input
						type="checkbox"
						checked={state.start_at_login}
						onChange={(event) => saveSettings(event.target.checked)}
					/>{" "}
					Start NOALBS Cloud Connect when my computer starts
				</label>
				<details>
					<summary>Advanced</summary>
					<label>
						API URL
						<input
							aria-label="API URL"
							value={apiUrl}
							onChange={(event) => setApiUrl(event.target.value)}
						/>
					</label>
					<button type="button" onClick={() => saveSettings()}>
						Save API URL
					</button>
				</details>
				{!state.start_at_login && (
					<p className="note">
						With start at login off, Connect cannot manage OBS until you open
						it.
					</p>
				)}
				<p className="note">
					Unpairing removes this PC's local token. Revoke access in the
					dashboard.
				</p>
			</section>
			{error && <p role="alert">{error}</p>}
		</main>
	);
}

const root = document.getElementById("root");
if (!root) throw new Error("root element missing");
ReactDOM.createRoot(root).render(
	<React.StrictMode>
		<App />
	</React.StrictMode>,
);
