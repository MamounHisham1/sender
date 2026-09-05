import React, { useCallback, useEffect, useState } from 'react';
import {
  ActivityIndicator,
  FlatList,
  Image,
  KeyboardAvoidingView,
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from 'react-native';
import { StatusBar } from 'expo-status-bar';
import { CameraView, useCameraPermissions } from 'expo-camera';
import { useSenderConn, ConnState, FeedItem } from './src/useSenderConn';
import { loadPairing, savePin, saveHost, laptopHostFromExpo, normalizeHost, Pairing } from './src/config';
import { parsePairPayload } from './src/pairing';
import { pickImage, makeImgMsg, copyText, copyImage } from './src/media';
import { newId } from './src/protocol';

export default function App() {
  const [pairing, setPairing] = useState<Pairing | null>(null);
  const [pinInput, setPinInput] = useState('');
  const [hostInput, setHostInput] = useState('');
  const [textInput, setTextInput] = useState('');
  const [showSetup, setShowSetup] = useState(false);
  const [deviceName] = useState(() => `Android-${Math.floor(1000 + Math.random() * 9000)}`);

  // Load saved pairing once; fall back to Expo's detected laptop host.
  useEffect(() => {
    (async () => {
      const p = await loadPairing();
      const host = p.host ?? laptopHostFromExpo();
      setPairing({ host, pin: p.pin });
      if (host) setHostInput(host);
    })();
  }, []);

  const host = pairing?.host ?? '';
  const pin = pairing?.pin ?? '';
  const conn = useSenderConn({ host, pin, deviceName });
  const ready = host.length > 0 && pin.length === 6;

  // ---- actions ----
  const submitSetup = useCallback(() => {
    const finalHost = normalizeHost(hostInput || laptopHostFromExpo() || '');
    if (!finalHost || !/^\d{6}$/.test(pinInput)) return;
    void savePin(pinInput);
    void saveHost(finalHost);
    setPairing({ host: finalHost, pin: pinInput });
  }, [hostInput, pinInput]);

  const sendText = useCallback(() => {
    const body = textInput.trim();
    if (!body) return;
    const msg = { type: 'text' as const, id: newId(), body, ts: Date.now() };
    if (conn.send(msg)) {
      conn.addLocal({ dir: 'out', kind: 'text', body, status: 'sent' });
      setTextInput('');
    }
  }, [textInput, conn]);

  const sendImage = useCallback(async (useCamera: boolean) => {
    const img = await pickImage(useCamera);
    if (!img) return;
    const msg = makeImgMsg(img);
    if (conn.send(msg)) {
      conn.addLocal({
        dir: 'out', kind: 'img',
        body: `data:${img.mime};base64,${img.base64}`,
        name: img.name, status: 'sent',
      });
    }
  }, [conn]);

  const copyItem = useCallback(async (item: FeedItem) => {
    if (item.kind === 'text') await copyText(item.body);
    else await copyImage(extractBase64(item.body), 'image/png');
  }, []);

  // ---- setup screen ----
  if (!ready || showSetup) {
    return (
      <SetupScreen
        pinInput={pinInput}
        hostInput={hostInput}
        setPinInput={setPinInput}
        setHostInput={setHostInput}
        onSubmit={() => { submitSetup(); setShowSetup(false); }}
        onScanned={(host, pin) => {
          void savePin(pin);
          void saveHost(host);
          setPairing({ host, pin });
          setShowSetup(false);
        }}
        detected={laptopHostFromExpo()}
      />
    );
  }

  // ---- main screen ----
  return (
    <View style={st.safe}>
      <StatusBar style="light" />
      <View style={st.header}>
        <View style={{ flex: 1 }}>
          <Text style={st.title}>Sender</Text>
          <Text style={st.subtitle}>{host}</Text>
        </View>
        <StateBadge state={conn.state} onPress={() => setShowSetup(true)} />
      </View>

      <FlatList
        style={st.feed}
        data={conn.feed}
        inverted
        keyExtractor={it => it.id}
        renderItem={({ item }) => <FeedRow item={item} onCopy={() => void copyItem(item)} />}
        ListEmptyComponent={
          <Text style={st.empty}>
            Connected. Anything you send shows on the laptop instantly — and lands in its clipboard.
            {'\n\n'}Long-press a bubble to copy it back to this phone's clipboard.
          </Text>
        }
      />

      <KeyboardAvoidingView behavior={Platform.OS === 'ios' ? 'padding' : undefined}>
        <View style={st.composer}>
          <Pressable style={st.iconBtn} onPress={() => void sendImage(false)}>
            <Text style={st.iconTxt}>🖼</Text>
          </Pressable>
          <Pressable style={st.iconBtn} onPress={() => void sendImage(true)}>
            <Text style={st.iconTxt}>📷</Text>
          </Pressable>
          <TextInput
            style={st.input}
            placeholder="text to laptop…"
            placeholderTextColor="#666"
            value={textInput}
            onChangeText={setTextInput}
            multiline
          />
          <Pressable style={st.sendBtn} onPress={sendText}>
            <Text style={st.sendTxt}>Send</Text>
          </Pressable>
        </View>
      </KeyboardAvoidingView>
    </View>
  );
}

// ---------- components ----------

const STATE_LABEL: Record<ConnState, string> = {
  idle: 'OFF',
  connecting: '···',
  authenticating: '···',
  open: 'LIVE',
  badpin: 'PIN?',
  unreachable: 'NO LINK',
  closed: 'OFF',
};

function StateBadge({ state, onPress }: { state: ConnState; onPress: () => void }) {
  const color =
    state === 'open' ? '#2ecc71' :
    state === 'badpin' ? '#e74c3c' :
    state === 'unreachable' ? '#e74c3c' :
    state === 'connecting' || state === 'authenticating' ? '#f39c12' : '#555';
  return (
    <Pressable onPress={onPress} style={[st.badge, { borderColor: color }]}>
      {state === 'connecting' || state === 'authenticating'
        ? <ActivityIndicator size="small" color={color} />
        : <Text style={[st.badgeTxt, { color }]}>{STATE_LABEL[state]}</Text>}
    </Pressable>
  );
}

function FeedRow({ item, onCopy }: { item: FeedItem; onCopy: () => void }) {
  const mine = item.dir === 'out';
  return (
    <Pressable onLongPress={onCopy}>
      <View style={[st.row, mine ? st.rowOut : st.rowIn]}>
        {item.kind === 'img' ? (
          <Image source={{ uri: item.body }} style={st.img} resizeMode="contain" />
        ) : (
          <Text style={[st.txt, mine && st.txtOut]}>{item.body}</Text>
        )}
        <Text style={st.time}>{new Date(item.ts).toLocaleTimeString()}</Text>
      </View>
    </Pressable>
  );
}

function SetupScreen(props: {
  pinInput: string;
  hostInput: string;
  setPinInput: (v: string) => void;
  setHostInput: (v: string) => void;
  onSubmit: () => void;
  onScanned: (host: string, pin: string) => void;
  detected: string | null;
}) {
  const { pinInput, hostInput, setPinInput, setHostInput, onSubmit, onScanned, detected } = props;
  const valid = /^\d{6}$/.test(pinInput) && (hostInput.length > 0 || !!detected);
  const [scanning, setScanning] = useState(false);
  const [permission, requestPermission] = useCameraPermissions();
  const [scanError, setScanError] = useState('');

  const startScan = useCallback(async () => {
    setScanError('');
    if (!permission?.granted) {
      const res = await requestPermission();
      if (!res.granted) {
        setScanError('Camera permission needed to scan the QR.');
        return;
      }
    }
    setScanning(true);
  }, [permission, requestPermission]);

  if (scanning) {
    return (
      <View style={st.scanWrap}>
        <StatusBar style="light" />
        <CameraView
          style={st.camera}
          facing="back"
          barcodeScannerSettings={{ barcodeTypes: ['qr'] }}
          onBarcodeScanned={res => {
            const parsed = parsePairPayload(res.data);
            if (parsed) {
              setScanning(false);
              onScanned(parsed.host, parsed.pin);
            } else {
              setScanError('That QR is not a Sender code — try again.');
            }
          }}
        />
        <View style={st.scanOverlay}>
          <Text style={st.scanHint}>Point at the QR on your laptop</Text>
          {!!scanError && <Text style={st.scanErr}>{scanError}</Text>}
          <Pressable style={st.cancelBtn} onPress={() => setScanning(false)}>
            <Text style={st.goTxt}>Cancel</Text>
          </Pressable>
        </View>
      </View>
    );
  }

  return (
    <ScrollView contentContainerStyle={st.setupWrap} keyboardShouldPersistTaps="handled">
      <StatusBar style="light" />
      <Text style={st.logo}>Sender</Text>
      <Text style={st.hint}>
        Scan the QR on your laptop — or enter the PIN manually.
        {'\n'}Laptop address is detected automatically over Wi-Fi.
      </Text>

      <Pressable style={st.scanBtn} onPress={() => void startScan()}>
        <Text style={st.scanBtnTxt}>📷 Scan laptop QR</Text>
      </Pressable>
      {!!scanError && <Text style={st.scanErrCenter}>{scanError}</Text>}

      <Text style={st.label}>Laptop address</Text>
      <TextInput
        style={st.field}
        value={hostInput}
        onChangeText={setHostInput}
        placeholder={detected ?? '192.168.1.20:8787'}
        placeholderTextColor="#666"
        autoCapitalize="none"
        autoCorrect={false}
        keyboardType="url"
      />

      <Text style={st.label}>Pairing PIN</Text>
      <TextInput
        style={[st.field, { fontSize: 28, letterSpacing: 8 }]}
        value={pinInput}
        onChangeText={v => setPinInput(v.replace(/\D/g, '').slice(0, 6))}
        placeholder="000000"
        placeholderTextColor="#666"
        keyboardType="number-pad"
      />

      <Pressable
        style={[st.goBtn, !valid && { opacity: 0.4 }]}
        disabled={!valid}
        onPress={onSubmit}
      >
        <Text style={st.goTxt}>Connect</Text>
      </Pressable>
    </ScrollView>
  );
}

function extractBase64(dataUri: string): string {
  const idx = dataUri.indexOf('base64,');
  return idx >= 0 ? dataUri.slice(idx + 7) : dataUri;
}

// ---------- styles ----------

const st = StyleSheet.create({
  safe: { flex: 1, backgroundColor: '#0e1116', paddingTop: 40 },
  header: { flexDirection: 'row', alignItems: 'center', paddingHorizontal: 16, paddingBottom: 10 },
  title: { color: '#fff', fontSize: 20, fontWeight: '700' },
  subtitle: { color: '#8aa', fontSize: 12 },
  badge: { borderWidth: 1, borderRadius: 14, paddingHorizontal: 12, paddingVertical: 6 },
  badgeTxt: { fontSize: 12, fontWeight: '700' },
  feed: { flex: 1, paddingHorizontal: 10 },
  empty: { color: '#667', textAlign: 'center', marginTop: 40, paddingHorizontal: 30, lineHeight: 20 },
  row: { maxWidth: '85%', borderRadius: 12, paddingVertical: 8, paddingHorizontal: 12, marginVertical: 4 },
  rowIn: { alignSelf: 'flex-start', backgroundColor: '#1c2330' },
  rowOut: { alignSelf: 'flex-end', backgroundColor: '#153a2c' },
  txt: { color: '#eef', fontSize: 15 },
  txtOut: { color: '#d6f5e5' },
  img: { width: 220, height: 220 },
  time: { color: '#556', fontSize: 10, alignSelf: 'flex-end', marginTop: 4 },
  composer: { flexDirection: 'row', alignItems: 'flex-end', padding: 8, gap: 6, backgroundColor: '#141a24' },
  iconBtn: { padding: 10, borderRadius: 10, backgroundColor: '#1c2330' },
  iconTxt: { fontSize: 20 },
  input: {
    flex: 1, minHeight: 42, maxHeight: 120, backgroundColor: '#1c2330', borderRadius: 10,
    color: '#fff', paddingHorizontal: 12, paddingTop: 10, fontSize: 15,
  },
  sendBtn: { backgroundColor: '#2ecc71', borderRadius: 10, paddingHorizontal: 16, paddingVertical: 12 },
  sendTxt: { color: '#04150c', fontWeight: '800' },
  setupWrap: { flexGrow: 1, justifyContent: 'center', padding: 28 },
  logo: { color: '#fff', fontSize: 34, fontWeight: '800', textAlign: 'center', marginBottom: 8 },
  hint: { color: '#8aa', textAlign: 'center', marginBottom: 30, lineHeight: 20 },
  label: { color: '#9ab', fontSize: 12, marginBottom: 6, marginTop: 14, textTransform: 'uppercase' },
  field: {
    backgroundColor: '#1c2330', borderRadius: 10, color: '#fff',
    paddingHorizontal: 14, paddingVertical: 12, fontSize: 16,
  },
  goBtn: { backgroundColor: '#2ecc71', borderRadius: 12, paddingVertical: 14, alignItems: 'center', marginTop: 30 },
  goTxt: { color: '#04150c', fontWeight: '800', fontSize: 16 },
  scanBtn: { backgroundColor: '#1c2330', borderRadius: 12, paddingVertical: 14, alignItems: 'center', marginBottom: 6, borderWidth: 1, borderColor: '#2ecc71' },
  scanBtnTxt: { color: '#2ecc71', fontWeight: '800', fontSize: 16 },
  scanErr: { color: '#e74c3c', textAlign: 'center', marginTop: 8 },
  scanErrCenter: { color: '#e74c3c', textAlign: 'center', marginTop: 10 },
  scanWrap: { flex: 1, backgroundColor: '#000' },
  camera: { flex: 1 },
  scanOverlay: { position: 'absolute', bottom: 0, left: 0, right: 0, padding: 24, alignItems: 'center', backgroundColor: 'rgba(0,0,0,0.55)' },
  scanHint: { color: '#fff', fontSize: 15, marginBottom: 12, textAlign: 'center' },
  cancelBtn: { backgroundColor: '#2ecc71', borderRadius: 10, paddingVertical: 12, paddingHorizontal: 32, marginTop: 8 },
});
