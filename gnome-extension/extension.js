import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

import {createPepperXClient} from './ipc.js';

const LOG_PREFIX = '[Pepper X]';
const STATUS_LABEL_PREFIX = 'Status';
const SETTINGS_ACTION_LABEL = 'Open Pepper X Settings';
const HISTORY_ACTION_LABEL = 'Open Pepper X History';
const STATUS_POLL_INTERVAL_MS = 500;

const ICON_READY = 'audio-input-microphone-symbolic';
const ICON_RECORDING = 'media-record-symbolic';
const ICON_WORKING = 'content-loading-symbolic';
const ICON_ERROR = 'dialog-warning-symbolic';
const ICON_DISCONNECTED = 'network-offline-symbolic';

const PepperXIndicator = GObject.registerClass(
class PepperXIndicator extends PanelMenu.Button {
    _init(onOpenSettings, onOpenHistory) {
        super._init(0.0, 'Pepper X');

        this._icon = new St.Icon({
            icon_name: ICON_READY,
            style_class: 'system-status-icon',
        });
        this.add_child(this._icon);

        this._statusItem = new PopupMenu.PopupMenuItem(`${STATUS_LABEL_PREFIX}: Connecting`, {
            reactive: false,
            can_focus: false,
        });
        this.menu.addMenuItem(this._statusItem);

        this._versionItem = new PopupMenu.PopupMenuItem('Pepper X v0.1.0', {
            reactive: false,
            can_focus: false,
        });
        this.menu.addMenuItem(this._versionItem);

        const settingsItem = new PopupMenu.PopupMenuItem(SETTINGS_ACTION_LABEL);
        settingsItem.connect('activate', () => onOpenSettings());
        this.menu.addMenuItem(settingsItem);

        const historyItem = new PopupMenu.PopupMenuItem(HISTORY_ACTION_LABEL);
        historyItem.connect('activate', () => onOpenHistory());
        this.menu.addMenuItem(historyItem);
    }

    setStatus(statusLabel) {
        this._statusItem.label.text = `${STATUS_LABEL_PREFIX}: ${statusLabel}`;
    }

    setIconForState(state) {
        let iconName;
        switch (state) {
        case 'recording':
            iconName = ICON_RECORDING;
            break;
        case 'transcribing':
        case 'cleaning-up':
            iconName = ICON_WORKING;
            break;
        case 'error':
            iconName = ICON_ERROR;
            break;
        case 'disconnected':
            iconName = ICON_DISCONNECTED;
            break;
        case 'ready':
        default:
            iconName = ICON_READY;
            break;
        }
        if (this._icon.icon_name !== iconName)
            this._icon.icon_name = iconName;
    }
});

export default class PepperXExtension extends Extension {
    enable() {
        this._client = createPepperXClient();
        this._indicator = new PepperXIndicator(
            () => this.showSettings(),
            () => this.showHistory()
        );

        Main.panel.addToStatusArea(this.uuid, this._indicator);
        this._bootstrapConnection();
        this._startStatusPolling();
    }

    disable() {
        if (this._statusPollId) {
            GLib.source_remove(this._statusPollId);
            this._statusPollId = 0;
        }
        this._indicator?.destroy();
        this._indicator = null;
        this._client = null;
        this._capabilities = null;
    }

    showSettings() {
        if (!this._client)
            return;

        try {
            this._client.showSettings();
        } catch (error) {
            console.error(`${LOG_PREFIX} Failed to open settings`, error);
        }
    }

    showHistory() {
        if (!this._client)
            return;

        try {
            this._client.showHistory();
        } catch (error) {
            console.error(`${LOG_PREFIX} Failed to open history`, error);
        }
    }

    _bootstrapConnection() {
        try {
            const reply = this._client.ping();
            if (reply !== 'pong') {
                this._indicator?.setStatus('Degraded');
                console.error(`${LOG_PREFIX} Unexpected Ping response: ${reply}`);
            }

            this._capabilities = this._client.getCapabilities();
            this._refreshIndicatorState();
            console.log(`${LOG_PREFIX} capabilities`, this._capabilities);
        } catch (error) {
            this._capabilities = null;
            this._setDisconnectedState();
            console.error(`${LOG_PREFIX} Failed to reach Pepper X app service`, error);
        }
    }

    _startStatusPolling() {
        this._statusPollId = GLib.timeout_add(
            GLib.PRIORITY_DEFAULT,
            STATUS_POLL_INTERVAL_MS,
            () => {
                this._refreshIndicatorState();
                return GLib.SOURCE_CONTINUE;
            }
        );
    }

    _refreshIndicatorState() {
        if (!this._client) {
            // Retry connection with a fresh proxy
            try {
                this._client = createPepperXClient();
                this._bootstrapConnection();
            } catch (error) {
                this._setDisconnectedState();
                return;
            }
        }

        try {
            if (!this._capabilities)
                this._capabilities = this._client.getCapabilities();

            const liveStatus = this._client.getLiveStatus();
            this._indicator?.setStatus(this._statusLabelFor(liveStatus));
            this._indicator?.setIconForState(liveStatus.state);
        } catch (error) {
            this._capabilities = null;
            this._client = null;
            this._setDisconnectedState();
            console.error(`${LOG_PREFIX} Failed to refresh Pepper X status`, error);
        }
    }

    _setDisconnectedState() {
        this._indicator?.setStatus('Disconnected');
        this._indicator?.setIconForState('disconnected');
    }

    _statusLabelFor(liveStatus) {
        switch (liveStatus.state) {
        case 'recording':
            return 'Recording...';
        case 'transcribing':
            return 'Transcribing...';
        case 'cleaning-up':
            return 'Cleaning up...';
        case 'clipboard-fallback':
            return liveStatus.detail || 'Copied to clipboard';
        case 'error':
            return liveStatus.detail || 'Error';
        case 'ready':
        default:
            return this._capabilities?.modifierOnlySupported ? 'Ready' : 'Degraded';
        }
    }

}
