import GObject from 'gi://GObject';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

import {createPepperXClient} from './ipc.js';
import {KeybindingRegistry} from './keybindings.js';

const LOG_PREFIX = '[Pepper X]';
const SETTINGS_ACTION_LABEL = 'Open Pepper X Settings';

const PepperXIndicator = GObject.registerClass(
class PepperXIndicator extends PanelMenu.Button {
    _init(onOpenSettings) {
        super._init(0.0, 'Pepper X');

        const icon = new St.Icon({
            icon_name: 'audio-input-microphone-symbolic',
            style_class: 'system-status-icon',
        });
        this.add_child(icon);

        const settingsItem = new PopupMenu.PopupMenuItem(SETTINGS_ACTION_LABEL);
        settingsItem.connect('activate', () => onOpenSettings());
        this.menu.addMenuItem(settingsItem);
    }
});

export default class PepperXExtension extends Extension {
    enable() {
        this._client = createPepperXClient();
        this._keybindings = new KeybindingRegistry();
        this._indicator = new PepperXIndicator(() => this.showSettings());

        Main.panel.addToStatusArea(this.uuid, this._indicator);
        this._keybindings.registerCleanup(() => {
            this._indicator?.destroy();
            this._indicator = null;
        });

        this._bootstrapConnection();
    }

    disable() {
        this._keybindings?.clear();
        this._keybindings = null;
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

    _bootstrapConnection() {
        try {
            const reply = this._client.ping();
            if (reply !== 'pong')
                console.error(`${LOG_PREFIX} Unexpected Ping response: ${reply}`);

            this._capabilities = this._client.getCapabilities();
        } catch (error) {
            this._capabilities = null;
            console.error(`${LOG_PREFIX} Failed to reach Pepper X app service`, error);
        }
    }
}
