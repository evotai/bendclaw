export interface FreezeInputMode {
  shouldCaptureInput: boolean
  shouldUseRawMode: boolean
  resumeOnLineInput: boolean
}

export function getFreezeInputMode(isActive: boolean, isFrozen: boolean): FreezeInputMode {
  if (!isActive) {
    return {
      shouldCaptureInput: false,
      shouldUseRawMode: false,
      resumeOnLineInput: false,
    }
  }

  if (isFrozen) {
    return {
      shouldCaptureInput: false,
      shouldUseRawMode: false,
      resumeOnLineInput: true,
    }
  }

  return {
    shouldCaptureInput: true,
    shouldUseRawMode: true,
    resumeOnLineInput: false,
  }
}
