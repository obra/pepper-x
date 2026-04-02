import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import Shell from 'gi://Shell';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

import {createPepperXClient} from './ipc.js';

// D-Bus interface for screenshot service (called by the app)
const SCREENSHOT_SERVICE_XML = `<node>
  <interface name="com.obra.PepperX.Screenshot">
    <method name="CaptureWindow">
      <arg name="filename" type="s" direction="in"/>
      <arg name="success" type="b" direction="out"/>
    </method>
  </interface>
</node>`;

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
        this._createStatusPill();
        this._bootstrapConnection();
        this._startStatusPolling();
        this._startScreenshotService();
    }

    disable() {
        this._destroyStatusPill();
        this._stopScreenshotService();
        if (this._statusPollId) {
            GLib.source_remove(this._statusPollId);
            this._statusPollId = 0;
        }
        this._indicator?.destroy();
        this._indicator = null;
        this._client = null;
        this._capabilities = null;
    }

    _createStatusPill() {
        this._pill = new St.BoxLayout({
            style_class: 'pepper-x-pill',
            style: 'background-color: rgba(30,30,30,0.9); border-radius: 999px; padding: 6px 18px; border: 1px solid rgba(255,255,255,0.15);',
            reactive: false,
            can_focus: false,
            track_hover: false,
            visible: false,
        });

        this._pillDot = new St.Label({
            text: '\u25CF',
            style: 'color: #e01b24; font-size: 14px; margin-right: 8px;',
        });

        this._pillSpinner = new St.Label({
            text: '\u25CE',
            style: 'color: #3584e4; font-size: 14px; margin-right: 8px;',
            visible: false,
        });

        this._pillLabel = new St.Label({
            text: '',
            style: 'color: white; font-size: 13px; font-weight: 500;',
        });

        this._pill.add_child(this._pillDot);
        this._pill.add_child(this._pillSpinner);
        this._pill.add_child(this._pillLabel);

        Main.layoutManager.addTopChrome(this._pill);
        this._positionPill();
    }

    _destroyStatusPill() {
        if (this._pill) {
            Main.layoutManager.removeChrome(this._pill);
            this._pill.destroy();
            this._pill = null;
        }
    }

    _positionPill() {
        if (!this._pill)
            return;
        const monitor = Main.layoutManager.primaryMonitor;
        if (!monitor)
            return;
        // Position at top center, below the panel
        const panelHeight = Main.panel?.height ?? 32;
        const x = Math.floor(monitor.x + monitor.width / 2 - this._pill.width / 2);
        const y = monitor.y + panelHeight + 8;
        this._pill.set_position(x, y);
    }

    _updateStatusPill(state) {
        if (!this._pill)
            return;

        const isBusy = state === 'recording' || state === 'transcribing' || state === 'cleaning-up';

        if (isBusy) {
            const isRecording = state === 'recording';
            this._pillDot.visible = isRecording;
            this._pillSpinner.visible = !isRecording;
            this._pillLabel.text = state === 'recording' ? 'Recording...'
                : state === 'transcribing' ? 'Transcribing...'
                : 'Cleaning up...';
            this._pill.visible = true;
            this._positionPill();
        } else {
            this._pill.visible = false;
        }
    }

    _startScreenshotService() {
        this._screenshotDbusId = Gio.DBus.session.register_object(
            '/com/obra/PepperX/Screenshot',
            Gio.DBusNodeInfo.new_for_xml(SCREENSHOT_SERVICE_XML)
                .lookup_interface('com.obra.PepperX.Screenshot'),
            (connection, sender, objectPath, interfaceName, methodName, parameters, invocation) => {
                if (methodName === 'CaptureWindow') {
                    const [filename] = parameters.deep_unpack();
                    // Call the Shell's own Screenshot D-Bus interface from within
                    // the extension (same process, so access is granted).
                    Gio.DBus.session.call(
                        'org.gnome.Shell.Screenshot',
                        '/org/gnome/Shell/Screenshot',
                        'org.gnome.Shell.Screenshot',
                        'ScreenshotWindow',
                        GLib.Variant.new('(bbbs)', [false, false, false, filename]),
                        GLib.VariantType.new('(bs)'),
                        Gio.DBusCallFlags.NONE,
                        5000,
                        null,
                        (conn, res) => {
                            try {
                                const reply = conn.call_finish(res);
                                const [success] = reply.deep_unpack();
                                invocation.return_value(GLib.Variant.new('(b)', [success]));
                            } catch (error) {
                                console.error(`${LOG_PREFIX} screenshot D-Bus call failed:`, error);
                                invocation.return_value(GLib.Variant.new('(b)', [false]));
                            }
                        }
                    );
                }
            },
            null,
            null,
        );
    }

    _stopScreenshotService() {
        if (this._screenshotDbusId) {
            Gio.DBus.session.unregister_object(this._screenshotDbusId);
            this._screenshotDbusId = 0;
        }
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
            this._updateStatusPill(liveStatus.state);
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
