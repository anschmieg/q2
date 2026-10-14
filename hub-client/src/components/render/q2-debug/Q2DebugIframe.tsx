import { useEffect, useRef, useState } from 'react';
import { getUserIdentity } from '../../../services/userSettings';

interface Q2DebugIframeProps {
  astJson: string;
  currentFilePath: string;
  onNavigateToDocument?: (path: string, anchor: string | null) => void;
  setAst: (newAst: any) => void;
  customComponentsCode?: Record<string, string>; // Component name -> transpiled JS code
}

/**
 * Wrapper component that renders the q2-debug Ast component in a
 * sandboxed iframe. Verbatim port of the original AstIframe with the
 * iframe `src` pointing at the renamed `/q2-debug.html` route.
 */
export function Q2DebugIframe({
  astJson,
  currentFilePath,
  onNavigateToDocument,
  setAst,
  customComponentsCode,
}: Q2DebugIframeProps) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [iframeReady, setIframeReady] = useState(false);
  // The user's name and colour, for the AST caret's flag in the iframe.
  const [user, setUser] = useState<{ name: string; color: string } | null>(null);

  useEffect(() => {
    let cancelled = false;
    getUserIdentity()
      .then((s) => { if (!cancelled) setUser({ name: s.userName, color: s.userColor }); })
      .catch(() => { /* no identity: the caret shows without a flag */ });
    return () => { cancelled = true; };
  }, []);

  // Handle messages from the iframe
  useEffect(() => {
    const handleMessage = (event: MessageEvent) => {
      // In production, verify event.origin for security
      if (event.data.type === 'IFRAME_READY') {
        setIframeReady(true);
      } else if (event.data.type === 'NAVIGATE_TO_DOCUMENT') {
        onNavigateToDocument?.(event.data.path, event.data.anchor);
      } else if (event.data.type === 'SET_AST') {
        setAst(event.data.ast);
      }
    };

    window.addEventListener('message', handleMessage);
    return () => window.removeEventListener('message', handleMessage);
  }, [onNavigateToDocument, setAst]);

  // Send custom components code when iframe is ready (only once or when it changes)
  useEffect(() => {
    if (!iframeReady || !iframeRef.current?.contentWindow) return;

    if (customComponentsCode) {
      iframeRef.current.contentWindow.postMessage(
        {
          type: 'LOAD_CUSTOM_COMPONENTS',
          componentsCode: customComponentsCode,
        },
        '*'
      );
    }
  }, [iframeReady, customComponentsCode]);

  // Send AST updates when iframe is ready
  useEffect(() => {
    if (!iframeReady || !iframeRef.current?.contentWindow) return;

    iframeRef.current.contentWindow.postMessage(
      {
        type: 'UPDATE_AST',
        payload: {
          astJson,
          currentFilePath,
          user,
        },
      },
      '*'
    );
  }, [iframeReady, astJson, currentFilePath, user]);

  return (
    <iframe
      ref={iframeRef}
      src="q2-debug.html"
      title="q2-debug Renderer"
      sandbox="allow-scripts allow-same-origin"
      style={{
        width: '100%',
        height: '100%',
        border: 'none',
        display: 'block',
      }}
    />
  );
}
