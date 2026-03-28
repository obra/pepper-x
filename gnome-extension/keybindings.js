export class KeybindingRegistry {
    constructor() {
        this._cleanups = [];
    }

    registerCleanup(cleanup) {
        if (typeof cleanup === 'function')
            this._cleanups.push(cleanup);
    }

    clear() {
        while (this._cleanups.length > 0) {
            const cleanup = this._cleanups.pop();
            cleanup();
        }
    }
}
