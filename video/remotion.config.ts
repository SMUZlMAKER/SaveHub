import { Config } from "@remotion/cli/config";

Config.setVideoImageFormat("jpeg");
Config.setOverwriteOutput(true);
// Тихая аудиодорожка: без звука Telegram показывает ролик как GIF.
Config.setEnforceAudioTrack(true);
