import * as ImagePicker from 'expo-image-picker';
import * as Clipboard from 'expo-clipboard';
import { Msg, newId } from './protocol';

export async function pickImage(useCamera: boolean): Promise<{ base64: string; mime: string; name: string } | null> {
  const perm = useCamera
    ? await ImagePicker.requestCameraPermissionsAsync()
    : await ImagePicker.requestMediaLibraryPermissionsAsync();
  if (!perm.granted) return null;

  const result = useCamera
    ? await ImagePicker.launchCameraAsync({ quality: 0.8, base64: true })
    : await ImagePicker.launchImageLibraryAsync({ quality: 0.8, base64: true });

  if (result.canceled || !result.assets?.[0]?.base64) return null;
  const a = result.assets[0];
  return {
    base64: a.base64!,
    mime: a.mimeType ?? 'image/jpeg',
    name: a.fileName ?? `photo-${Date.now()}.jpg`,
  };
}

export function makeImgMsg(img: { base64: string; mime: string; name: string }): Msg {
  return {
    type: 'img',
    id: newId(),
    name: img.name,
    mime: img.mime,
    data: img.base64,
    ts: Date.now(),
  };
}

export async function copyText(text: string): Promise<void> {
  await Clipboard.setStringAsync(text);
}

export async function copyImage(base64: string, mime: string): Promise<void> {
  // expo-clipboard supports PNG/JPEG via setImageAsync(base64)
  await Clipboard.setImageAsync(base64);
}
