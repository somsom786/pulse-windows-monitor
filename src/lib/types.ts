export type SensorStatus = "ok" | "unsupported" | "needs_admin" | "unavailable";
export interface Reading {
  value: number | null;
  status: SensorStatus;
  note: string | null;
}

export interface CpuSnap {
  name: string;
  logical: number;
  physical: number;
  total: number;
  perCore: number[];
  freqMhz: number | null;
  baseMhz: number | null;
  perCoreFreq: (number | null)[];
}

export interface PageFileSnap {
  name: string;
  total: number;
  inUse: number;
  peak: number;
}

export interface MemSnap {
  total: number;
  available: number;
  inUse: number;
  modified: number | null;
  standby: number | null;
  free: number | null;
  compressed: number | null;
  commitTotal: number;
  commitLimit: number;
  commitPeak: number;
  kernelPaged: number;
  kernelNonpaged: number;
  systemCache: number;
  pagefiles: PageFileSnap[];
  pagesInPerSec: number | null;
  pagesOutPerSec: number | null;
  handles: number;
  processes: number;
  threads: number;
  pressure: "normal" | "elevated" | "high" | "critical";
  pressureReason: string;
}

export interface GpuSnap {
  name: string;
  driver: string;
  util: Reading;
  memCtrlUtil: Reading;
  vramUsed: Reading;
  vramTotal: Reading;
  temp: Reading;
  tempSlowdown: Reading;
  hotspot: Reading;
  fanPct: Reading;
  fanRpm: Reading;
  powerW: Reading;
  powerLimitW: Reading;
  clockGpu: Reading;
  clockMem: Reading;
  clockGpuMax: Reading;
  clockMemMax: Reading;
  pstate: string | null;
  throttle: string[];
  throttleNote: string;
}

export interface DiskSnap {
  letter: string;
  label: string;
  fs: string;
  kind: string;
  total: number;
  free: number;
  readBps: number | null;
  writeBps: number | null;
  activePct: number | null;
  latencyMs: number | null;
}

export interface AdapterRate {
  name: string;
  rxBps: number;
  txBps: number;
  linkBps: number;
}

export interface SystemSnap {
  ts: number;
  uptimeS: number;
  cpu: CpuSnap;
  cpuTemp: Reading;
  cpuPower: Reading;
  mem: MemSnap;
  gpu: GpuSnap | null;
  gpuStatus: string;
  disks: DiskSnap[];
  diskIo: { readBps: number | null; writeBps: number | null; activePct: number | null };
  net: { rxBps: number | null; txBps: number | null; adapters: AdapterRate[] };
  pollMs: number;
  sampleCostMs: number;
}

export interface ProcRow {
  pid: number;
  ppid: number;
  name: string;
  path: string;
  createTime: string; // FILETIME as a decimal string (exceeds 2^53)
  workingSet: number;
  privateWs: number;
  commit: number;
  peakWs: number;
  cpu: number;
  ioReadBps: number;
  ioWriteBps: number;
  gpu: number | null;
  gpuMem: number | null;
  threads: number;
  handles: number;
  session: number;
}

export interface TopApp {
  name: string;
  count: number;
  mem: number;
  cpu: number;
  gpu: number;
  gpuMem: number;
}

export interface Settings {
  pollIntervalMs: number;
  recordEnabled: boolean;
  recordIntervalS: number;
  procSnapshotIntervalS: number;
  retentionDays: number;
  monitorInBackground: boolean;
  minimizeToTray: boolean;
  tempUnit: "C" | "F";
  theme: "dark" | "light" | "system";
  notifications: boolean;
  alertMemEnabled: boolean;
  alertMemPct: number;
  alertMemSustainS: number;
  alertDiskEnabled: boolean;
  alertDiskFreePct: number;
  alertGpuTempEnabled: boolean;
  alertGpuTempC: number;
  alertCpuEnabled: boolean;
  alertCpuPct: number;
  alertCpuSustainS: number;
  alertCooldownMin: number;
  spikeMemGb: number;
}

export interface Point {
  t: number;
  cpu: number | null;
  cpuMax: number | null;
  ramUsed: number | null;
  ramUsedMax: number | null;
  ramAvailMin: number | null;
  commit: number | null;
  gpuUtil: number | null;
  gpuUtilMax: number | null;
  vramUsed: number | null;
  vramUsedMax: number | null;
  gpuTemp: number | null;
  gpuPower: number | null;
  cpuTemp: number | null;
  diskR: number | null;
  diskW: number | null;
  netRx: number | null;
  netTx: number | null;
}

export interface SnapProc {
  pid: number;
  name: string;
  privateWs: number;
  workingSet: number;
  commit: number;
  cpu: number;
  gpu: number | null;
  gpuMem: number | null;
}
export interface AppAgg {
  name: string;
  count: number;
  privateWs: number;
  cpu: number;
  gpuMem: number | null;
}
export interface SnapAt {
  ts: number;
  requested: number;
  procs: SnapProc[];
  apps: AppAgg[];
}
export interface SpikeRow {
  id: number;
  ts: number;
  kind: string;
  value: number;
  baseline: number | null;
  summary: string;
  detail: any;
}
export interface AlertRow {
  id: number;
  ts: number;
  kind: string;
  message: string;
}
export interface PeakRow {
  name: string;
  peakPrivateWs: number;
  peakTs: number;
  maxInstances: number;
  peakCpu: number;
  peakGpuMem: number | null;
}
export interface GrowthRow {
  pid: number;
  name: string;
  startWs: number;
  nowWs: number;
  growthPerHour: number;
  minutes: number;
  samples: number;
  note: string;
}
export interface StoreInfo {
  path: string;
  sizeBytes: number;
  samples: number;
  snapshots: number;
  spikes: number;
  oldestTs: number | null;
  newestTs: number | null;
}
export interface Attention {
  severity: "info" | "warn" | "crit";
  title: string;
  detail: string;
  ts: number | null;
  view: string | null;
}

// ---- storage ----
export interface ScanMeta {
  id: string;
  root: string;
  startedUnix: number;
  durationMs: number;
  files: number;
  dirs: number;
  totalLogical: number;
  totalAlloc: number;
  errors: number;
  volumeTotal: number;
  volumeFree: number;
  hardlinkSaved: number;
  hardlinkFiles: number;
  cancelled: boolean;
  fsName: string;
}
export interface ScanProgress {
  root: string;
  files: number;
  dirs: number;
  bytes: number;
  errors: number;
  current: string;
  elapsedMs: number;
  queued: number;
}
export interface Drive {
  letter: string;
  label: string;
  fs: string;
  kind: string;
  total: number;
  free: number;
}
export interface Row {
  id: number;
  parent: number;
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  alloc: number;
  files: number;
  cat: number;
  flags: number;
  mtime: number;
  ext: string;
}
export interface Tm {
  id: number;
  name: string;
  value: number;
  alloc: number;
  size: number;
  cat: number;
  isDir: boolean;
  other: number;
  files: number;
  flags: number;
  children?: Tm[];
}
export interface Children {
  node: Row;
  rows: Row[];
  total: number;
}
export interface Crumb {
  id: number;
  name: string;
}
export interface SearchResult {
  rows: Row[];
  matched: number;
  matchedBytes: number;
}
export interface Filter {
  name?: string;
  exts?: string[];
  cats?: number[];
  minSize?: number;
  maxSize?: number;
  folder?: number;
  kind?: "files" | "dirs" | "all";
  olderThanDays?: number;
  metric?: string;
  sort?: string;
  desc?: boolean;
  limit?: number;
  offset?: number;
}
export interface ExtStat {
  ext: string;
  count: number;
  alloc: number;
  size: number;
  cat: number;
}
export interface CatStat {
  cat: number;
  name: string;
  count: number;
  alloc: number;
  size: number;
}
export interface Unaccounted {
  volumeUsed: number;
  scannedAlloc: number;
  gap: number;
  coversWholeVolume: boolean;
  notes: string[];
}
export interface Overview {
  meta: ScanMeta;
  topFiles: Row[];
  topFolders: Row[];
  exts: ExtStat[];
  cats: CatStat[];
  unaccounted: Unaccounted;
  errorSamples: { path: string; code: number }[];
}
export interface Location {
  label: string;
  group: string;
  path: string;
  node: number | null;
  alloc: number;
  size: number;
  files: number;
  risk: "safe" | "review" | "system";
  what: string;
  cleanup: string;
}
export interface App {
  name: string;
  publisher: string;
  version: string;
  location: string;
  size: number;
  source: string;
}
export interface DiffRow {
  path: string;
  before: number;
  after: number;
  delta: number;
  depth: number;
}
export interface Diff {
  rows: DiffRow[];
  totalBefore: number;
  totalAfter: number;
  older: string;
  newer: string;
}
export interface DupGroup {
  size: number;
  wasted: number;
  files: Row[];
}
export interface DupProgress {
  stage: string;
  done: number;
  total: number;
}

// ---- hardware ----
export interface HwSensor {
  name: string;
  kind: string;
  value: number;
  min: number | null;
  max: number | null;
  identifier: string;
  parent: string;
}
export interface HubData {
  source: string;
  status: string;
  sensors: HwSensor[];
  updatedMs: number;
}
export interface PhysDisk {
  model: string;
  media: string;
  bus: string;
  size: number;
  health: string;
  firmware: string;
  serial: string;
  letters: string[];
  temperature: number | null;
  wear: number | null;
  powerOnHours: number | null;
  readErrors: number | null;
  writeErrors: number | null;
  reliabilityStatus: string;
}
export interface NetAdapter {
  name: string;
  description: string;
  status: string;
  linkSpeedBps: number | null;
  mac: string;
  ipv4: string[];
  ipv6: string[];
  gateway: string[];
  dns: string[];
  dhcp: boolean | null;
}
export interface Inventory {
  os: string;
  osBuild: string;
  machine: string;
  board: string;
  bios: string;
  cpu: string;
  ramDesc: string;
  ramModules: string[];
  disks: PhysDisk[];
  adapters: NetAdapter[];
}
export interface AppInfo {
  version: string;
  dataDir: string;
  isAdmin: boolean;
  pid: number;
  cpuName: string;
}
