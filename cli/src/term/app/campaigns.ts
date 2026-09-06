import { campaignFingerprint, createAdSlotState, triggerAdSlot, type AdContent, type AdSlotState } from '../viewmodel/ad-slot.js'

interface CampaignNotice { id: string; kind: string; priority?: number; title: string; body_md?: string }

/** Unknown catalog kinds are retained on the wire but not shown as campaigns. */
export function campaignContent(notices: CampaignNotice[]): AdContent[] {
  return notices.flatMap(notice => notice.kind === 'notice' || notice.kind === 'ad'
    ? [{ id: notice.id, kind: notice.kind, priority: notice.priority, title: notice.title, body: notice.body_md ?? '' }]
    : [])
}

/** Reconcile a new campaign catalog without resetting session rotation state.
 * The host supplies entitlement and time; no auth/native reads happen here. */
export function refreshCampaigns(state: AdSlotState, fresh: AdContent[], premium: boolean, now: number): void {
  const previousById = new Map([...state.notices, ...state.ads].map(campaign => [campaign.id, campaign]))
  const keep = {
    seenNoticeIds: state.seenNoticeIds,
    triggered: state.triggered,
    currentId: state.currentId,
    shownAt: state.shownAt,
    rotationDueAt: state.rotationDueAt,
    queuedId: state.queuedId,
    shownFingerprints: state.shownFingerprints,
  }
  Object.assign(state, createAdSlotState(fresh, { premium, shownFingerprints: state.shownFingerprints }), keep)
  const showing = [...state.notices, ...state.ads].find(campaign => campaign.id === state.currentId)
  if (state.currentId && !showing) {
    state.currentId = null
    state.queuedId = null
  } else if (showing) {
    const before = previousById.get(showing.id)
    if (before && campaignFingerprint(before) !== campaignFingerprint(showing)) {
      state.shownAt = now
      state.queuedId = null
    }
  }
  if (!state.triggered && (state.notices.length > 0 || state.ads.length > 0)) triggerAdSlot(state, now)
}
