import AxeBuilder from '@axe-core/playwright';
import { expect, test } from '@playwright/test';

test('@a11y critical first-run and empty-workspace flows remain accessible', async ({ page }) => {
  test.setTimeout(60_000);
  await page.goto('/');

  await test.step('1. 首次注册', async () => {
    await expect(page.getByRole('heading', { name: '先创建你的账号' })).toBeVisible();
    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations.filter(({ impact }) => impact === 'serious' || impact === 'critical')).toEqual([]);
    await page.getByLabel('显示名称').fill('前端验收用户');
    await page.getByLabel('登录名').fill('frontend-e2e');
    await page.getByLabel('密码').fill('frontend-e2e-password');
    await page.getByRole('button', { name: '创建并进入 iMail' }).click();
    await expect(page.getByRole('heading', { name: '还没有接入邮箱' })).toBeVisible();
  });

  await test.step('2. 搜索和邮件筛选', async () => {
    const search = page.getByPlaceholder('搜索当前范围内的邮件');
    await search.fill('验收搜索');
    await expect(search).toHaveValue('验收搜索');
    await page.getByRole('button', { name: '未读', exact: true }).click();
    await page.getByRole('button', { name: '有附件', exact: true }).click();
    await page.getByRole('button', { name: '全部', exact: true }).click();
    await search.fill('');
  });

  await test.step('3. 添加邮箱弹窗', async () => {
    await page.getByRole('main').getByRole('button', { name: '添加邮箱' }).click();
    await expect(page.getByRole('heading', { name: '添加邮箱' })).toBeVisible();
    await page.getByRole('button', { name: '关闭添加邮箱窗口' }).click();
  });

  await test.step('4. 写邮件空状态', async () => {
    await page.getByRole('button', { name: '写邮件' }).click();
    await expect(page.getByRole('heading', { name: '先接入一个真实邮箱' })).toBeVisible();
    await page.getByRole('button', { name: '关闭写信' }).click();
  });

  await test.step('5. 设置无障碍', async () => {
    await page.getByRole('button', { name: '打开设置' }).click();
    await expect(page.getByRole('dialog')).toBeVisible();
    await expect(page.getByRole('heading', { name: '通用' })).toBeVisible();
    await expect(page.getByRole('button', { name: /同步健康/ })).toHaveCount(0);
    await page.locator('.overlay').evaluate(async (element) => {
      await Promise.all(element.getAnimations({ subtree: true }).map((animation) => animation.finished));
    });
    const settingsA11y = await new AxeBuilder({ page }).analyze();
    expect(settingsA11y.violations.filter(({ impact }) => impact === 'serious' || impact === 'critical')).toEqual([]);
  });

  await test.step('6. 快捷键录制可取消', async () => {
    await page.getByRole('button', { name: /快捷键/ }).click();
    await expect(page.getByRole('heading', { name: '快捷键' })).toBeVisible();
    const binding = page.locator('.shortcut-row button').first();
    await binding.click();
    await expect(binding).toHaveText('请按键…');
    await binding.press('Escape');
    await expect(binding).not.toHaveText('请按键…');
  });

  await test.step('7. 隐私清除的两阶段确认可安全取消', async () => {
    await page.getByRole('button', { name: /隐私与数据/ }).click();
    await page.getByRole('button', { name: '开始清除…' }).click();
    await expect(page.getByText('第一次确认：核对清除范围')).toBeVisible();
    await page.getByRole('button', { name: '我已了解，继续验证' }).click();
    await expect(page.getByText('第二次确认：验证当前身份')).toBeVisible();
    await page.getByRole('button', { name: '取消' }).click();
    await page.getByRole('button', { name: '关闭设置' }).click();
  });

  await test.step('8. 外部接入工作区', async () => {
    await page.getByRole('button', { name: /外部接入/ }).click();
    await expect(page.getByRole('heading', { name: 'MCP Agent 接入' })).toBeVisible();
  });

  await test.step('9. 退出并重新登录', async () => {
    await page.getByRole('button', { name: '前端验收用户，打开账号菜单' }).click();
    await page.getByRole('menuitem', { name: /切换账号/ }).click();
    await expect(page.getByRole('heading', { name: '欢迎回来' })).toBeVisible();
    await expect(page.getByLabel('登录名')).toHaveValue('frontend-e2e');
    await page.getByLabel('密码').fill('frontend-e2e-password');
    await page.getByRole('button', { name: '登录 iMail' }).click();
    await expect(page.getByRole('heading', { name: '还没有接入邮箱' })).toBeVisible();
    const workspaceA11y = await new AxeBuilder({ page }).analyze();
    expect(workspaceA11y.violations.filter(({ impact }) => impact === 'serious' || impact === 'critical')).toEqual([]);
  });
});
