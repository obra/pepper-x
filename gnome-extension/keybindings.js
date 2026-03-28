import Clutter from 'gi://Clutter';

const MODIFIER_ONLY_KEY_SYMBOLS = new Set([
    Clutter.KEY_Control_L,
    Clutter.KEY_Control_R,
]);

export class KeybindingRegistry {
    constructor() {
        this._cleanups = [];
        this._pressedModifiers = new Set();
    }

    registerCleanup(cleanup) {
        if (typeof cleanup === 'function')
            this._cleanups.push(cleanup);
    }

    registerModifierHold(onStart, onStop, stage = global.stage) {
        const handlerId = stage.connect('captured-event', (_actor, event) => {
            const eventType = event.type();
            if (eventType !== Clutter.EventType.KEY_PRESS &&
                eventType !== Clutter.EventType.KEY_RELEASE)
                return Clutter.EVENT_PROPAGATE;

            const keySymbol = event.get_key_symbol();
            if (!MODIFIER_ONLY_KEY_SYMBOLS.has(keySymbol))
                return Clutter.EVENT_PROPAGATE;

            if (eventType === Clutter.EventType.KEY_PRESS) {
                const wasActive = this._pressedModifiers.size > 0;
                this._pressedModifiers.add(keySymbol);
                if (!wasActive)
                    onStart();
            } else {
                const wasActive = this._pressedModifiers.size > 0;
                this._pressedModifiers.delete(keySymbol);
                if (wasActive && this._pressedModifiers.size === 0)
                    onStop();
            }

            return Clutter.EVENT_PROPAGATE;
        });

        this.registerCleanup(() => {
            this._pressedModifiers.clear();
            stage.disconnect(handlerId);
        });
    }

    clear() {
        while (this._cleanups.length > 0) {
            const cleanup = this._cleanups.pop();
            cleanup();
        }

        this._pressedModifiers.clear();
    }
}
