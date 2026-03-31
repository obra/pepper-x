import Gio from 'gi://Gio';

export const SERVICE_NAME = 'com.obra.PepperX.Service';
export const OBJECT_PATH = '/com/obra/PepperX';
export const INTERFACE_NAME = 'com.obra.PepperX';

const INTERFACE_XML = `<node>
  <interface name="com.obra.PepperX">
    <method name="Ping">
      <arg name="reply" type="s" direction="out" />
    </method>
    <method name="StartRecording">
      <arg name="trigger_source" type="s" direction="in" />
    </method>
    <method name="StopRecording" />
    <method name="ShowSettings" />
    <method name="ShowHistory" />
    <method name="GetCapabilities">
      <arg name="modifier_only_supported" type="b" direction="out" />
      <arg name="extension_connected" type="b" direction="out" />
      <arg name="version" type="s" direction="out" />
    </method>
    <method name="GetLiveStatus">
      <arg name="state" type="s" direction="out" />
      <arg name="detail" type="s" direction="out" />
    </method>
  </interface>
</node>`;

const PepperXProxy = Gio.DBusProxy.makeProxyWrapper(INTERFACE_XML);

export class PepperXClient {
    constructor(proxy) {
        this._proxy = proxy;
    }

    ping() {
        const [reply] = this._proxy.PingSync();
        return reply;
    }

    startRecording(triggerSource) {
        this._proxy.StartRecordingSync(triggerSource);
    }

    stopRecording() {
        this._proxy.StopRecordingSync();
    }

    showSettings() {
        this._proxy.ShowSettingsSync();
    }

    showHistory() {
        this._proxy.ShowHistorySync();
    }

    getCapabilities() {
        const [modifierOnlySupported, extensionConnected, version] =
            this._proxy.GetCapabilitiesSync();

        return {
            modifierOnlySupported,
            extensionConnected,
            version,
        };
    }

    getLiveStatus() {
        const [state, detail] = this._proxy.GetLiveStatusSync();

        return {state, detail};
    }
}

export function createPepperXClient(connection = Gio.DBus.session) {
    const proxy = new PepperXProxy(connection, SERVICE_NAME, OBJECT_PATH);
    return new PepperXClient(proxy);
}
