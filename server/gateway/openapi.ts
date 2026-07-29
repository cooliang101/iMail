const errorResponse = (description: string) => ({
  description,
  content: { 'application/json': { schema: { $ref: '#/components/schemas/ErrorResponse' } } },
});

const pageParameters = [
  { name: 'limit', in: 'query', description: '每页数量，默认 25，最大 100', schema: { type: 'integer', minimum: 1, maximum: 100, default: 25 } },
  { name: 'cursor', in: 'query', description: '上一页返回的不透明游标', schema: { type: 'string' } },
  { name: 'mailboxRole', in: 'query', description: '邮箱文件夹角色', schema: { type: 'string', enum: ['inbox', 'sent', 'archive', 'trash', 'custom'] } },
  { name: 'unread', in: 'query', description: '按未读状态筛选', schema: { type: 'boolean' } },
  { name: 'since', in: 'query', description: '包含该时间之后的邮件', schema: { type: 'string', format: 'date-time' } },
  { name: 'before', in: 'query', description: '包含该时间之前的邮件', schema: { type: 'string', format: 'date-time' } },
  { name: 'q', in: 'query', description: '搜索主题、摘要和发件人', schema: { type: 'string', maxLength: 200 } },
];

export const gatewayOpenApi = {
  openapi: '3.1.0',
  info: {
    title: 'iMail Developer Gateway',
    version: '1.0.0',
    description: '使用短期 Token 按邮箱地址安全读取与发送邮件。接口不接受或返回 iMail 内部邮箱 ID。',
  },
  servers: [{ url: '/gateway/v1', description: '当前 iMail 实例' }],
  tags: [
    { name: 'System', description: '网关状态' },
    { name: 'Mailboxes', description: 'Token 被授权使用的邮箱' },
    { name: 'Messages', description: '邮件列表、详情与附件' },
    { name: 'Delivery', description: '发送邮件' },
  ],
  components: {
    securitySchemes: {
      bearerAuth: { type: 'http', scheme: 'bearer', bearerFormat: 'iMail Token', description: '开发者网关创建的 imail_ 开头短期 Token' },
    },
    schemas: {
      Address: {
        type: 'object', required: ['name', 'address'],
        properties: { name: { type: 'string' }, address: { type: 'string', format: 'email' } },
      },
      Attachment: {
        type: 'object', required: ['filename', 'contentType', 'size', 'index'],
        properties: { filename: { type: 'string' }, contentType: { type: 'string' }, size: { type: 'integer' }, index: { type: 'integer' } },
      },
      Mailbox: {
        type: 'object', required: ['email', 'provider', 'displayName', 'group', 'status'],
        properties: {
          email: { type: 'string', format: 'email' }, provider: { type: 'string' }, displayName: { type: 'string' }, group: { type: 'string' },
          status: { type: 'string', enum: ['connected', 'syncing', 'error'] }, lastSyncAt: { type: 'string', format: 'date-time' },
        },
      },
      MessageSummary: {
        type: 'object', required: ['id', 'accountEmail', 'folder', 'mailboxRole', 'from', 'to', 'subject', 'preview', 'date', 'unread', 'flagged', 'hasAttachments', 'attachments', 'labels'],
        properties: {
          id: { type: 'string' }, accountEmail: { type: 'string', format: 'email' }, folder: { type: 'string' },
          mailboxRole: { type: 'string', enum: ['inbox', 'sent', 'archive', 'trash', 'custom'] }, from: { $ref: '#/components/schemas/Address' },
          to: { type: 'array', items: { $ref: '#/components/schemas/Address' } }, subject: { type: 'string' }, preview: { type: 'string' },
          date: { type: 'string', format: 'date-time' }, unread: { type: 'boolean' }, flagged: { type: 'boolean' }, hasAttachments: { type: 'boolean' },
          attachments: { type: 'array', items: { $ref: '#/components/schemas/Attachment' } }, labels: { type: 'array', items: { type: 'string' } },
        },
      },
      MessageDetail: {
        allOf: [
          { $ref: '#/components/schemas/MessageSummary' },
          { type: 'object', required: ['text'], properties: { text: { type: 'string' }, html: { type: 'string' } } },
        ],
      },
      Page: {
        type: 'object', required: ['limit', 'count', 'hasMore', 'nextCursor'],
        properties: { limit: { type: 'integer' }, count: { type: 'integer' }, hasMore: { type: 'boolean' }, nextCursor: { type: ['string', 'null'] } },
      },
      ErrorResponse: {
        type: 'object', required: ['error'],
        properties: { error: { type: 'object', required: ['code', 'message', 'requestId'], properties: { code: { type: 'string' }, message: { type: 'string' }, requestId: { type: 'string' }, details: {} } } },
      },
    },
  },
  paths: {
    '/health': {
      get: { tags: ['System'], summary: '检查网关状态', operationId: 'gatewayHealth', security: [], responses: { '200': { description: '网关正常' } } },
    },
    '/mailboxes': {
      get: {
        tags: ['Mailboxes'], summary: '列出已授权邮箱', operationId: 'listMailboxes', security: [{ bearerAuth: [] }],
        responses: { '200': { description: '邮箱列表', content: { 'application/json': { schema: { type: 'object', properties: { mailboxes: { type: 'array', items: { $ref: '#/components/schemas/Mailbox' } } } } } } }, '401': errorResponse('Token 无效或权限不足') },
      },
    },
    '/messages': {
      get: {
        tags: ['Messages'], summary: '查询邮件', description: '返回不含正文的邮件摘要，使用 nextCursor 继续翻页。', operationId: 'listMessages', security: [{ bearerAuth: [] }],
        parameters: [{ name: 'mailbox', in: 'query', description: '邮箱地址；省略时查询 Token 的全部邮箱', schema: { type: 'string', format: 'email' } }, ...pageParameters],
        responses: {
          '200': { description: '邮件分页结果', content: { 'application/json': { schema: { type: 'object', properties: { messages: { type: 'array', items: { $ref: '#/components/schemas/MessageSummary' } }, page: { $ref: '#/components/schemas/Page' } } } } } },
          '400': errorResponse('参数或游标无效'), '401': errorResponse('Token 无效或权限不足'), '404': errorResponse('邮箱不存在或未授权'),
        },
      },
    },
    '/mailboxes/{mailbox}/messages': {
      get: {
        tags: ['Messages'], summary: '查询指定邮箱的邮件', operationId: 'listMailboxMessages', security: [{ bearerAuth: [] }],
        parameters: [{ name: 'mailbox', in: 'path', required: true, description: '邮箱地址', schema: { type: 'string', format: 'email' } }, ...pageParameters],
        responses: { '200': { description: '邮件分页结果' }, '400': errorResponse('参数无效'), '401': errorResponse('Token 无效或权限不足'), '404': errorResponse('邮箱不存在或未授权') },
      },
    },
    '/messages/{messageId}': {
      get: {
        tags: ['Messages'], summary: '读取邮件详情', operationId: 'getMessage', security: [{ bearerAuth: [] }],
        parameters: [{ name: 'messageId', in: 'path', required: true, schema: { type: 'string' } }],
        responses: { '200': { description: '包含正文的邮件详情', content: { 'application/json': { schema: { type: 'object', properties: { message: { $ref: '#/components/schemas/MessageDetail' } } } } } }, '401': errorResponse('Token 无效或权限不足'), '404': errorResponse('邮件不存在或未授权') },
      },
    },
    '/messages/{messageId}/attachments/{index}': {
      get: {
        tags: ['Messages'], summary: '下载邮件附件', operationId: 'downloadAttachment', security: [{ bearerAuth: [] }],
        parameters: [{ name: 'messageId', in: 'path', required: true, schema: { type: 'string' } }, { name: 'index', in: 'path', required: true, schema: { type: 'integer', minimum: 0 } }],
        responses: { '200': { description: '附件二进制内容', content: { 'application/octet-stream': {} } }, '401': errorResponse('Token 无效或权限不足'), '404': errorResponse('邮件或附件不存在') },
      },
    },
    '/send': {
      post: {
        tags: ['Delivery'], summary: '发送邮件', operationId: 'sendMessage', security: [{ bearerAuth: [] }],
        requestBody: {
          required: true, content: { 'application/json': {
            schema: { type: 'object', additionalProperties: false, required: ['mailbox', 'to', 'subject', 'text'], properties: { mailbox: { type: 'string', format: 'email' }, to: { type: 'array', items: { type: 'string', format: 'email' } }, cc: { type: 'array', items: { type: 'string', format: 'email' } }, subject: { type: 'string' }, text: { type: 'string' }, html: { type: 'string' } } },
            example: { mailbox: 'sender@example.com', to: ['recipient@example.com'], subject: 'Hello from iMail', text: 'Sent through the local gateway.' },
          } },
        },
        responses: { '201': { description: '发送成功' }, '400': errorResponse('请求正文无效'), '401': errorResponse('Token 无效或权限不足'), '404': errorResponse('发件邮箱不存在或未授权') },
      },
    },
  },
} as const;
