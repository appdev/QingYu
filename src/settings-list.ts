// 设置里的分组列表。尺寸、缩进、分隔线和点击高亮在 settings.css 的 .client-list 里按平台处理，
// 调用处只管内容。

export type SettingsListButton = {
  title: string;
  detail?: string;
  /** 右侧显示 › ，用于进入下一级页面 */
  disclosure?: boolean;
  /** 红色文字，用于退出登录、删除等操作 */
  destructive?: boolean;
  onSelect: () => void;
};

export function settingsList(...rows: HTMLElement[]) {
  const list = document.createElement('div');
  list.className = 'client-list';
  list.append(...rows);
  return list;
}

export function settingsListButton({ title, detail, disclosure, destructive, onSelect }: SettingsListButton) {
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'settings-row';
  button.classList.toggle('settings-disclosure', Boolean(disclosure));
  button.classList.toggle('client-sign-out', Boolean(destructive));
  const label = document.createElement('span'); label.className = 'settings-label';
  const heading = document.createElement('span'); heading.textContent = title;
  label.append(heading);
  if (detail) {
    const small = document.createElement('small'); small.textContent = detail;
    label.append(small);
  }
  button.append(label);
  button.onclick = onSelect;
  return button;
}
