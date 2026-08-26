import { useEffect, useRef, useState, useCallback } from 'react';
import { Msg, encode, decode, newId } from './protocol';
import { wsUrlFor } from './config';

export type ConnState =
  | 'idle'
  | 'connecting'
  | 'authenticating'
  | 'open'
  | 'badpin'
  | 'unreachable'
  | 'closed';

export interface FeedItem {
  id: string;
  dir: 'in' | 'out';
  kind: 'text' | 'img';
  body: string;      // text content, or data: URI for images
  name?: string;     // image filename
  ts: number;
  status?: 'sent' | 'failed';
}

interface Options {
  host: string;
  pin: string;
  deviceName: string;
}

/**
 * Owns the WebSocket lifecycle: connect, PIN handshake, auto-reconnect with
 * backoff, and the message feed for the UI.
 */
export function useSenderConn({ host, pin, deviceName }: Options) {
  const [state, setState] = useState<ConnState>('idle');
  const [feed, setFeed] = useState<FeedItem[]>([]);
  const wsRef = useRef<WebSocket | null>(null);
  const backoffRef = useRef(500);
  const failCountRef = useRef(0);
  const genRef = useRef(0); // invalidates stale reconnect loops
  const stoppedRef = useRef(false);

  const push = useCallback((item: Omit<FeedItem, 'id' | 'ts'> & { id?: string }) => {
    setFeed(prev => [{ ...item, id: item.id ?? newId(), ts: Date.now() }, ...prev].slice(0, 100));
  }, []);

  const send = useCallback((msg: Msg): boolean => {
    const ws = wsRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) return false;
    try {
      ws.send(encode(msg));
      return true;
    } catch {
      return false;
    }
  }, []);

  const connect = useCallback(() => {
    const gen = ++genRef.current;
    setState('connecting');

    let ws: WebSocket;
    try {
      ws = new WebSocket(wsUrlFor(host));
    } catch {
      scheduleReconnect(gen);
      return;
    }
    wsRef.current = ws;

    ws.onopen = () => {
      if (gen !== genRef.current) return;
      failCountRef.current = 0;
      setState('authenticating');
      // Server demands hello{pin} as the very first message.
      ws.send(encode({ type: 'hello', pin, name: deviceName }));
    };

    ws.onmessage = ev => {
      if (gen !== genRef.current) return;
      const msg = typeof ev.data === 'string' ? decode(ev.data) : null;
      if (!msg) return;
      switch (msg.type) {
        case 'welcome': {
          if (msg.ok) {
            backoffRef.current = 500;
            failCountRef.current = 0;
            setState('open');
          } else {
            stoppedRef.current = true; // wrong PIN: stop retrying
            setState('badpin');
            try { ws.close(); } catch {}
          }
          break;
        }
        case 'text': {
          push({ dir: 'in', kind: 'text', body: msg.body });
          break;
        }
        case 'img': {
          const uri = `data:${msg.mime};base64,${msg.data}`;
          push({ dir: 'in', kind: 'img', body: uri, name: msg.name });
          break;
        }
        default:
          break;
      }
    };

    ws.onclose = () => {
      if (gen !== genRef.current || stoppedRef.current) return;
      scheduleReconnect(gen);
    };

    ws.onerror = () => {
      try { ws.close(); } catch {}
    };

    function scheduleReconnect(myGen: number) {
      if (myGen !== genRef.current || stoppedRef.current) return;
      failCountRef.current += 1;
      // After a few failures the link itself is dead — say so, keep retrying quietly.
      if (failCountRef.current >= 3) setState('unreachable');
      const delay = backoffRef.current;
      backoffRef.current = Math.min(backoffRef.current * 2, 8000);
      setTimeout(() => {
        if (myGen === genRef.current && !stoppedRef.current) connect();
      }, delay);
    }
  }, [host, pin, deviceName, push]);

  const disconnect = useCallback(() => {
    genRef.current++;
    stoppedRef.current = true;
    try { wsRef.current?.close(); } catch {}
    wsRef.current = null;
    setState('closed');
  }, []);

  useEffect(() => {
    if (!host || !pin) return;
    stoppedRef.current = false;
    connect();
    return () => {
      genRef.current++;
      stoppedRef.current = true;
      try { wsRef.current?.close(); } catch {}
    };
  }, [host, pin, deviceName, connect]);

  return { state, feed, send, reconnectNow: connect, disconnect, addLocal: push };
}
