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

const PepperXIndicator = GObject.registerClass(
class PepperXIndicator extends PanelMenu.Button {
    _init(onOpenSettings, onOpenHistory) {
        super._init(0.0, 'Pepper X');

        const icon = new St.Icon({
            icon_name: 'audio-input-microphone-symbolic',
            style_class: 'system-status-icon',
        });
        this.add_child(icon);

        this._statusItem = new PopupMenu.PopupMenuItem(`${STATUS_LABEL_PREFIX}: Connecting`, {
            reactive: false,
            can_focus: false,
        });
        this.menu.addMenuItem(this._statusItem);

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
    }

    disable() {
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
            if (!this._capabilities.modifierOnlySupported)
                this._indicator?.setStatus('Degraded');
            else
                this._indicator?.setStatus('Ready');
            console.log(`${LOG_PREFIX} capabilities`, this._capabilities);
        } catch (error) {
            this._capabilities = null;
            this._indicator?.setStatus('Disconnected');
            console.error(`${LOG_PREFIX} Failed to reach Pepper X app service`, error);
        }
    }
}
