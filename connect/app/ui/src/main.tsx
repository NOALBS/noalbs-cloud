import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import React from "react";
import ReactDOM from "react-dom/client";
import "./style.css";

type ObsConfig = {
	install_kind: "native" | "flatpak" | "custom_binary";
	binary_path: string | null;
	config_dir_override: string | null;
	portable: boolean;
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
	obs_process_running: boolean;
	obs: ObsConfig;
	detected_binary: string;
	config_dir: string;
};
type State = {
	platform: string;
	start_at_login: boolean;
	api_url: string;
	dashboard_url: string | null;
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
type Tone = "ok" | "busy" | "bad" | "off";

const SETTINGS = "settings";

const OBS_STATES: Record<string, [string, Tone]> = {
	connected: ["OBS connected", "ok"],
	starting: ["Starting OBS…", "busy"],
	waiting_for_input: ["OBS is waiting for input", "busy"],
	safe_mode: ["OBS is asking about Safe Mode", "busy"],
	websocket_disabled: ["OBS websocket is off", "busy"],
	not_responding: ["OBS is not responding", "bad"],
	crashed: ["OBS crashed", "bad"],
	not_running: ["OBS not running", "off"],
	unknown: ["OBS running, not started by Connect", "off"],
};

function obsStatus(state: string): [string, Tone] {
	return OBS_STATES[state] ?? [state.replaceAll("_", " "), "off"];
}

const INSTALL_KINDS: Record<ObsConfig["install_kind"], string> = {
	native: "Installed app",
	flatpak: "Flatpak",
	custom_binary: "Custom location",
};

function Dot({ tone }: { tone: Tone }) {
	return <span className={`dot ${tone}`} aria-hidden="true" />;
}

function Row({
	label,
	children,
}: {
	label: string;
	children: React.ReactNode;
}) {
	return (
		<div className="row">
			<span className="row-label">{label}</span>
			<span className="row-value">{children}</span>
		</div>
	);
}

function Field({
	label,
	hint,
	children,
}: {
	label: string;
	hint?: string;
	children: React.ReactNode;
}) {
	return (
		// biome-ignore lint/a11y/noLabelWithoutControl: the control is passed as children
		<label className="field">
			<span className="field-label">{label}</span>
			<span className="field-control">
				{children}
				{hint && <span className="hint">{hint}</span>}
			</span>
		</label>
	);
}

function Check({
	checked,
	onChange,
	children,
}: {
	checked: boolean;
	onChange: (checked: boolean) => void;
	children: React.ReactNode;
}) {
	return (
		<label className="check">
			<span>{children}</span>
			<input
				type="checkbox"
				role="switch"
				aria-checked={checked}
				checked={checked}
				onChange={(event) => onChange(event.target.checked)}
			/>
		</label>
	);
}

function App() {
	const [state, setState] = React.useState<State | null>(null);
	const [selected, setSelected] = React.useState<string | null>(null);
	const [name, setName] = React.useState("");
	const [adding, setAdding] = React.useState(false);
	const [error, setError] = React.useState("");
	const [pairing, setPairing] = React.useState<Pairing | null>(null);
	const [pairingSetupId, setPairingSetupId] = React.useState<string | null>(
		null,
	);
	const [pairingStatus, setPairingStatus] = React.useState("");
	const [apiUrl, setApiUrl] = React.useState("");
	const [dashboardUrl, setDashboardUrl] = React.useState("");
	const [firstRunStartAtLogin, setFirstRunStartAtLogin] = React.useState(true);
	const [editing, setEditing] = React.useState<Setup | null>(null);
	const [secondsLeft, setSecondsLeft] = React.useState(0);

	const applyState = React.useCallback((value: State) => {
		setState(value);
		setApiUrl(value.api_url);
		setDashboardUrl(value.dashboard_url ?? "");
	}, []);

	React.useEffect(() => {
		invoke<State>("get_state")
			.then((value) => {
				applyState(value);
				if (value.startup_error) setError(value.startup_error);
			})
			.catch((e) => setError(String(e)));
		const unlisteners = [
			listen<Pairing>("pairing-created", (event) => setPairing(event.payload)),
			listen<string>("pairing-status", (event) =>
				setPairingStatus(event.payload),
			),
			listen<State>("connect-state-changed", (event) => {
				applyState(event.payload);
				setPairing(null);
				setPairingSetupId(null);
			}),
			listen<string>("connect-error", (event) => setError(event.payload)),
		];
		return () => {
			for (const unlisten of unlisteners) void unlisten.then((fn) => fn());
		};
	}, [applyState]);

	React.useEffect(() => {
		if (!pairing) return;
		setSecondsLeft(pairing.expires_in);
		const countdown = window.setInterval(
			() => setSecondsLeft((seconds) => Math.max(0, seconds - 1)),
			1000,
		);
		return () => window.clearInterval(countdown);
	}, [pairing]);

	React.useEffect(() => {
		if (!state) return;
		const exists =
			selected === SETTINGS || state.setups.some((s) => s.id === selected);
		if (!exists) setSelected(state.setups[0]?.id ?? null);
	}, [state, selected]);

	async function run(action: () => Promise<State | undefined>) {
		setError("");
		try {
			const updated = await action();
			if (updated) applyState(updated);
		} catch (e) {
			setError(String(e));
		}
	}

	async function addSetup() {
		const trimmed = name.trim();
		if (!trimmed) return;
		await run(async () => {
			if (state?.first_run)
				await invoke("save_settings", {
					startAtLogin: firstRunStartAtLogin,
					apiUrl,
					dashboardUrl: dashboardUrl.trim() ? dashboardUrl : null,
				});
			const before = new Set(state?.setups.map((s) => s.id));
			const updated = await invoke<State>("add_setup", { name: trimmed });
			const created = updated.setups.find((s) => !before.has(s.id));
			if (created) setSelected(created.id);
			setName("");
			setAdding(false);
			return updated;
		});
	}

	async function saveSetup() {
		if (!editing) return;
		await run(async () => {
			const updated = await invoke<State>("save_setup", {
				setupId: editing.id,
				name: editing.name,
				obs: editing.obs,
			});
			setEditing(null);
			return updated;
		});
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

	function saveSettings(startAtLogin = state?.start_at_login ?? true) {
		return run(() =>
			invoke<State>("save_settings", {
				startAtLogin,
				apiUrl,
				dashboardUrl: dashboardUrl.trim() ? dashboardUrl : null,
			}),
		);
	}

	const setupAction = (
		command:
			| "start_obs"
			| "stop_obs"
			| "restart_obs"
			| "unpair_setup"
			| "enable_websocket",
		id: string,
	) => run(() => invoke<State>(command, { setupId: id }));

	if (!state)
		return (
			<div className="loading">
				{error ? <p role="alert">{error}</p> : <p>Loading…</p>}
			</div>
		);

	if (state.first_run && state.setups.length === 0)
		return (
			<div className="welcome">
				<div className="welcome-card">
					<img className="welcome-logo" src="/icon.png" alt="" />
					<h1>Welcome to NOALBS Cloud Connect</h1>
					<p className="muted">
						Connect links OBS on this computer to NOALBS Cloud. It works
						alongside the browser source and can also start, stop and restart
						OBS for you.
					</p>
					<Field label="Setup name" hint="For example the streamer's name.">
						<input
							// biome-ignore lint/a11y/noAutofocus: first field of the first-run screen
							autoFocus
							value={name}
							placeholder="My stream"
							onChange={(event) => setName(event.target.value)}
							onKeyDown={(event) => event.key === "Enter" && addSetup()}
						/>
					</Field>
					<Check
						checked={firstRunStartAtLogin}
						onChange={setFirstRunStartAtLogin}
					>
						Start NOALBS Cloud Connect when my computer starts
					</Check>
					<div className="buttons end">
						<button
							type="button"
							className="primary"
							disabled={!name.trim()}
							onClick={addSetup}
						>
							Continue
						</button>
					</div>
					{error && (
						<p className="error" role="alert">
							{error}
						</p>
					)}
				</div>
			</div>
		);

	const setup = state.setups.find((s) => s.id === selected) ?? null;
	const connectedCount = state.setups.filter(
		(s) => s.obs_state === "connected",
	).length;

	return (
		<div className="window">
			<header className="toolbar">
				<div className="toolbar-title">
					<strong>NOALBS Cloud Connect</strong>
					<span className="muted">
						{state.setups.length === 1
							? "1 setup"
							: `${state.setups.length} setups`}
						{" · "}
						{connectedCount} connected to OBS
					</span>
				</div>
			</header>
			<div className="panes">
				<nav className="sidebar">
					<div className="sidebar-heading">Setups</div>
					<ul className="list">
						{state.setups.map((s) => {
							const [label, tone] = obsStatus(s.obs_state);
							return (
								<li key={s.id}>
									<button
										type="button"
										className={`item ${selected === s.id ? "selected" : ""}`}
										onClick={() => {
											setSelected(s.id);
											setEditing(null);
										}}
									>
										<Dot tone={s.paired ? tone : "off"} />
										<span className="item-text">
											<span className="item-title">{s.name}</span>
											<span className="item-sub">
												{s.paired ? label : "Not paired"}
											</span>
										</span>
									</button>
								</li>
							);
						})}
					</ul>
					{adding ? (
						<div className="add-row">
							<input
								// biome-ignore lint/a11y/noAutofocus: opened by the user
								autoFocus
								aria-label="Setup name"
								placeholder="Setup name"
								value={name}
								onChange={(event) => setName(event.target.value)}
								onKeyDown={(event) => {
									if (event.key === "Enter") addSetup();
									if (event.key === "Escape") setAdding(false);
								}}
							/>
							<button type="button" onClick={addSetup}>
								Add
							</button>
						</div>
					) : (
						<button
							type="button"
							className="sidebar-action"
							onClick={() => setAdding(true)}
						>
							+ Add setup
						</button>
					)}
					<div className="sidebar-spacer" />
					<button
						type="button"
						className={`item ${selected === SETTINGS ? "selected" : ""}`}
						onClick={() => setSelected(SETTINGS)}
					>
						<span className="gear" aria-hidden="true">
							⚙
						</span>
						<span className="item-title">Settings</span>
					</button>
				</nav>
				<main className="detail">
					{error && (
						<div className="banner" role="alert">
							<span>{error}</span>
							<button
								type="button"
								className="link"
								onClick={() => setError("")}
							>
								Dismiss
							</button>
						</div>
					)}
					{selected === SETTINGS ? (
						<>
							<h1>Settings</h1>
							<section className="group">
								<div className="group-body">
									<Check
										checked={state.start_at_login}
										onChange={(checked) => saveSettings(checked)}
									>
										Start NOALBS Cloud Connect when my computer starts
									</Check>
									{!state.start_at_login && (
										<p className="hint">
											While Connect is closed, the watchdog and dashboard
											controls don't work.
										</p>
									)}
								</div>
							</section>
							<h2>Advanced</h2>
							<section className="group">
								<div className="group-body">
									<Field label="API URL">
										<input
											value={apiUrl}
											onChange={(event) => setApiUrl(event.target.value)}
										/>
									</Field>
									<Field
										label="Dashboard URL"
										hint="Only needed when the dashboard has its own address."
									>
										<input
											value={dashboardUrl}
											placeholder="Same as the API"
											onChange={(event) => setDashboardUrl(event.target.value)}
										/>
									</Field>
									<div className="buttons end">
										<button type="button" onClick={() => saveSettings()}>
											Save
										</button>
									</div>
								</div>
							</section>
						</>
					) : setup ? (
						<SetupDetail
							setup={setup}
							platform={state?.platform ?? "linux"}
							editing={editing?.id === setup.id ? editing : null}
							pairing={pairingSetupId === setup.id ? pairing : null}
							pairingActive={pairingSetupId === setup.id}
							pairingStatus={pairingStatus}
							secondsLeft={secondsLeft}
							onAction={(command) => setupAction(command, setup.id)}
							onPair={() => startPairing(setup.id)}
							onOpenBrowser={() =>
								invoke("open_verification_url", { setupId: setup.id }).catch(
									(e) => setError(String(e)),
								)
							}
							onEdit={() => setEditing(structuredClone(setup))}
							onEditChange={setEditing}
							onObsChange={updateObs}
							onSave={saveSetup}
							onCancel={() => setEditing(null)}
						/>
					) : (
						<div className="empty">
							<p>No setups yet.</p>
							<button type="button" onClick={() => setAdding(true)}>
								Add setup
							</button>
						</div>
					)}
				</main>
			</div>
		</div>
	);
}

function SetupDetail({
	setup,
	platform,
	editing,
	pairing,
	pairingActive,
	pairingStatus,
	secondsLeft,
	onAction,
	onPair,
	onOpenBrowser,
	onEdit,
	onEditChange,
	onObsChange,
	onSave,
	onCancel,
}: {
	setup: Setup;
	platform: string;
	editing: Setup | null;
	pairing: Pairing | null;
	pairingActive: boolean;
	pairingStatus: string;
	secondsLeft: number;
	onAction: (
		command:
			| "start_obs"
			| "stop_obs"
			| "restart_obs"
			| "unpair_setup"
			| "enable_websocket",
	) => void;
	onPair: () => void;
	onOpenBrowser: () => void;
	onEdit: () => void;
	onEditChange: (setup: Setup) => void;
	onObsChange: <K extends keyof ObsConfig>(key: K, value: ObsConfig[K]) => void;
	onSave: () => void;
	onCancel: () => void;
}) {
	const [obsLabel, obsTone] = obsStatus(setup.obs_state);
	const running = setup.obs_process_running;
	return (
		<>
			<h1>{setup.name}</h1>
			<p className="status-line">
				<Dot tone={setup.paired ? "ok" : "off"} />
				{setup.paired ? "Paired with NOALBS Cloud" : "Not paired"}
			</p>

			<h2>OBS</h2>
			<section className="group">
				<Row label="Status">
					<Dot tone={obsTone} />
					{obsLabel}
				</Row>
				<Row label="Install">{INSTALL_KINDS[setup.obs.install_kind]}</Row>
				{setup.obs.last_obs_version && (
					<Row label="Version">{setup.obs.last_obs_version}</Row>
				)}
				<div className="group-footer buttons">
					<button
						type="button"
						disabled={running}
						onClick={() => onAction("start_obs")}
					>
						Start
					</button>
					<button
						type="button"
						disabled={!running}
						onClick={() => onAction("stop_obs")}
					>
						Stop
					</button>
					<button
						type="button"
						disabled={!running}
						onClick={() => onAction("restart_obs")}
					>
						Restart
					</button>
					{setup.obs_state === "websocket_disabled" && (
						<button type="button" onClick={() => onAction("enable_websocket")}>
							Turn on OBS websocket
						</button>
					)}
				</div>
			</section>

			<h2>NOALBS Cloud</h2>
			<section className="group">
				{pairingActive ? (
					<div className="pairing">
						<p>Enter this code on the NOALBS Cloud pairing page:</p>
						<div className="code">{pairing?.user_code ?? "…"}</div>
						<div className="buttons center">
							<button
								type="button"
								className="primary"
								disabled={!pairing}
								onClick={onOpenBrowser}
							>
								Open pairing page
							</button>
						</div>
						<p className="hint">
							{pairingStatus || "Waiting for confirmation…"}
							{pairing &&
								` · Expires in ${Math.floor(secondsLeft / 60)}:${String(
									secondsLeft % 60,
								).padStart(2, "0")}`}
						</p>
					</div>
				) : (
					<>
						<Row label="Status">
							{setup.paired ? "Paired" : "Not paired yet"}
						</Row>
						<div className="group-footer buttons">
							<button
								type="button"
								className={setup.paired ? "" : "primary"}
								onClick={onPair}
							>
								{setup.paired ? "Pair again" : "Pair with NOALBS Cloud"}
							</button>
							{setup.paired && (
								<button type="button" onClick={() => onAction("unpair_setup")}>
									Unpair
								</button>
							)}
						</div>
						{setup.paired && (
							<p className="group-note">
								Unpairing removes this computer's token. To revoke access
								completely, also remove it in the dashboard.
							</p>
						)}
					</>
				)}
			</section>

			<h2>Setup</h2>
			<section className="group">
				{editing ? (
					<SetupForm
						setup={editing}
						platform={platform}
						onChange={onEditChange}
						onObsChange={onObsChange}
						onSave={onSave}
						onCancel={onCancel}
					/>
				) : (
					<>
						<Row label="OBS program">
							<span className="mono">
								{setup.obs.binary_path ?? setup.detected_binary}
							</span>
						</Row>
						<Row label="OBS settings folder">
							<span className="mono">
								{setup.obs.config_dir_override ?? setup.config_dir}
							</span>
						</Row>
						<Row label="Start OBS with Connect">
							{setup.obs.start_obs_with_connect ? "On" : "Off"}
						</Row>
						<div className="group-footer buttons">
							<button type="button" onClick={onEdit}>
								Edit…
							</button>
						</div>
					</>
				)}
			</section>
		</>
	);
}

function SetupForm({
	setup,
	platform,
	onChange,
	onObsChange,
	onSave,
	onCancel,
}: {
	setup: Setup;
	platform: string;
	onChange: (setup: Setup) => void;
	onObsChange: <K extends keyof ObsConfig>(key: K, value: ObsConfig[K]) => void;
	onSave: () => void;
	onCancel: () => void;
}) {
	const obs = setup.obs;
	return (
		<div className="group-body">
			<Field label="Name">
				<input
					value={setup.name}
					onChange={(event) => onChange({ ...setup, name: event.target.value })}
				/>
			</Field>
			<Field label="OBS install">
				<select
					value={obs.install_kind}
					onChange={(event) =>
						onObsChange(
							"install_kind",
							event.target.value as ObsConfig["install_kind"],
						)
					}
				>
					{Object.entries(INSTALL_KINDS)
						.filter(([value]) => value !== "flatpak" || platform === "linux")
						.map(([value, label]) => (
							<option key={value} value={value}>
								{label}
							</option>
						))}
				</select>
			</Field>
			<Field label="OBS program">
				<input
					value={obs.binary_path ?? setup.detected_binary}
					onChange={(event) =>
						onObsChange("binary_path", event.target.value.trim() || null)
					}
				/>
			</Field>
			{platform === "windows" && (
				<div className="checks">
					<Check
						checked={obs.portable}
						onChange={(checked) => onObsChange("portable", checked)}
					>
						Portable OBS (keeps its own settings next to obs64.exe)
					</Check>
				</div>
			)}
			<Field
				label="OBS settings folder"
				hint={
					platform === "windows"
						? "Normal OBS installs share their settings. Use a portable OBS copy for separate settings."
						: platform === "macos"
							? "OBS uses one settings folder. Use shared config with a distinct websocket port for multiple setups."
							: "Use a separate folder for each setup on this computer."
				}
			>
				<input
					disabled={platform === "macos" || platform === "windows"}
					value={obs.config_dir_override ?? setup.config_dir}
					onChange={(event) =>
						onObsChange(
							"config_dir_override",
							event.target.value.trim() || null,
						)
					}
				/>
			</Field>
			{(
				[
					["profile", "Profile"],
					["collection", "Scene collection"],
					["scene", "Scene"],
				] as const
			).map(([key, label]) => (
				<Field key={key} label={label}>
					<input
						value={obs[key] ?? ""}
						placeholder="OBS default"
						onChange={(event) => onObsChange(key, event.target.value || null)}
					/>
				</Field>
			))}
			<div className="checks">
				{(
					[
						["start_obs_with_connect", "Start OBS when Connect starts"],
						["minimize_to_tray", "Start OBS minimized to the tray"],
						["disable_updater", "Turn off the OBS updater"],
						[
							"use_dashboard_obs_settings",
							"Use the websocket settings from the dashboard",
						],
						[
							"shared_config",
							"Share the OBS settings folder with another setup",
						],
					] as const
				).map(([key, label]) => (
					<Check
						key={key}
						checked={obs[key]}
						onChange={(checked) => onObsChange(key, checked)}
					>
						{label}
					</Check>
				))}
			</div>
			{obs.shared_config && (
				<>
					<p className="hint">
						Each setup that shares a settings folder needs its own websocket
						port.
					</p>
					<Field label="Websocket port">
						<input
							type="number"
							value={obs.websocket_port ?? ""}
							onChange={(event) =>
								onObsChange(
									"websocket_port",
									event.target.value ? Number(event.target.value) : null,
								)
							}
						/>
					</Field>
					<Field label="Websocket password">
						<input
							type="password"
							value={obs.websocket_password_override ?? ""}
							onChange={(event) =>
								onObsChange(
									"websocket_password_override",
									event.target.value || null,
								)
							}
						/>
					</Field>
				</>
			)}
			<div className="buttons end">
				<button type="button" onClick={onCancel}>
					Cancel
				</button>
				<button type="button" className="primary" onClick={onSave}>
					Save
				</button>
			</div>
		</div>
	);
}

const root = document.getElementById("root");
if (!root) throw new Error("root element missing");
ReactDOM.createRoot(root).render(
	<React.StrictMode>
		<App />
	</React.StrictMode>,
);
