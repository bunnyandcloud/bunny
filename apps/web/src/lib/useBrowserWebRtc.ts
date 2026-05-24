import { useCallback, useEffect, useRef, useState } from 'react';
import {
  browserWebrtcCandidate,
  browserWebrtcOffer,
  browserWebrtcStop,
  getWebRtcConfig,
  sessionRealtimeWsUrl,
} from './api';

export function useBrowserWebRtc(sessionId: string,
  browserId: string | null,
) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const pcRef = useRef<RTCPeerConnection | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const [connected, setConnected] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const disconnect = useCallback(async () => {
    setConnected(false);
    wsRef.current?.close();
    wsRef.current = null;
    pcRef.current?.close();
    pcRef.current = null;
    if (videoRef.current) {
      videoRef.current.srcObject = null;
    }
  }, []);

  const connect = useCallback(async () => {
    if (!browserId || connecting) return;
    setConnecting(true);
    setError(null);
    try {
      await disconnect();
      const config = await getWebRtcConfig();
      if (!config.enabled) {
        throw new Error('WebRTC sidecar disabled');
      }

      const iceServers: RTCIceServer[] = config.ice_servers.map((s) => ({
        urls: s.urls,
        ...(s.username ? { username: s.username } : {}),
        ...(s.credential ? { credential: s.credential } : {}),
      }));

      const pc = new RTCPeerConnection({ iceServers });
      pcRef.current = pc;

      pc.ontrack = (ev) => {
        if (ev.track.kind === 'video' && videoRef.current) {
          videoRef.current.srcObject = ev.streams[0] ?? new MediaStream([ev.track]);
          setConnected(true);
        }
      };

      pc.onicecandidate = (ev) => {
        if (!ev.candidate || !browserId) return;
        void browserWebrtcCandidate(browserId, ev.candidate.toJSON()).catch(() => {});
      };

      const ws = new WebSocket(sessionRealtimeWsUrl(sessionId));
      wsRef.current = ws;
      ws.onmessage = (msg) => {
        try {
          const data = JSON.parse(msg.data as string) as {
            type?: string;
            candidate?: RTCIceCandidateInit;
          };
          if (data.type === 'webrtc.ice' && data.candidate) {
            void pc.addIceCandidate(data.candidate).catch(() => {});
          }
        } catch {
          /* ignore */
        }
      };

      pc.addTransceiver('video', { direction: 'recvonly' });
      const offer = await pc.createOffer();
      await pc.setLocalDescription(offer);
      const answer = await browserWebrtcOffer(browserId, {
        type: offer.type ?? 'offer',
        sdp: offer.sdp ?? '',
      });
      await pc.setRemoteDescription(new RTCSessionDescription(answer));
    } catch (e) {
      setError(String(e));
      setConnected(false);
      await disconnect();
    } finally {
      setConnecting(false);
    }
  }, [browserId, connecting, disconnect, sessionId]);

  useEffect(() => {
    return () => {
      if (browserId) {
        void browserWebrtcStop(browserId).catch(() => {});
      }
      void disconnect();
    };
  }, [browserId, disconnect]);

  return { videoRef, connected, connecting, error, connect, disconnect };
}
