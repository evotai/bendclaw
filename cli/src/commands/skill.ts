export { resolveSkillsDirs } from './skill/paths.js'
export { getSkillEntries, type SkillEntry } from './skill/scan.js'
export { skillList, skillListFromDirs, skillListView, type SkillListOptions } from './skill/list.js'
export {
  skillInstall,
  skillRemove,
  skillUpdate,
  startOfficialSkillSync,
  syncOfficialSkills,
  type ManageOptions,
  type OfficialSyncResult,
} from './skill/manage.js'
export {
  renderNotice,
  renderOperation,
  renderProgress,
  renderRemoved,
  renderSkillInventoryLines,
  renderSkillList,
  tildify,
  type OperationView,
  type SkillListView,
  type SkillOutcome,
  type SkillUnitView,
  type UnitNote,
  type UnitResult,
} from './skill/render.js'
