/**
 * TEMPORARY DEBUGGING AID — remove before shipping.
 *
 * Fixed, always-visible toggle strip inside the preview document for the
 * churn-debugging switches. Deliberately plain and obviously debug-y.
 */
import React from 'react';
import { setDebugSettings, useDebugSettings } from './debugSettings';

const barStyle: React.CSSProperties = {
    position: 'fixed',
    top: 8,
    right: 8,
    zIndex: 100000,
    padding: '6px 10px',
    background: '#111',
    color: '#0f0',
    font: '12px/1.4 monospace',
    border: '2px dashed #0f0',
    borderRadius: 4,
    display: 'flex',
    gap: 12,
    alignItems: 'center',
    userSelect: 'none',
};

export function DebugBar() {
    const s = useDebugSettings();
    return (
        <div style={barStyle} data-q2-owns-focus="" data-q2-debug-ignore="">
            <strong>DEBUG</strong>
            <label style={{ cursor: 'pointer' }}>
                <input
                    type="checkbox"
                    checked={s.contentKeys}
                    onChange={(e) => setDebugSettings({ contentKeys: e.currentTarget.checked })}
                />{' '}
                content keys {s.contentKeys ? '(new)' : '(old: index)'}
            </label>
            <label style={{ cursor: 'pointer' }}>
                <input
                    type="checkbox"
                    checked={s.churnColors}
                    onChange={(e) => setDebugSettings({ churnColors: e.currentTarget.checked })}
                />{' '}
                churn colors
            </label>
            <label style={{ cursor: 'pointer' }}>
                <input
                    type="checkbox"
                    checked={s.domFlash}
                    onChange={(e) => setDebugSettings({ domFlash: e.currentTarget.checked })}
                />{' '}
                DOM flash
            </label>
        </div>
    );
}
