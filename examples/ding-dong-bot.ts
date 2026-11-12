import { types } from 'wechaty-puppet'
import qrTerminal from 'qrcode-terminal'
import PuppetNeo from 'wechaty-puppet-neo'

const puppet = new PuppetNeo()

puppet.on('scan', ({ qrcode, status }) => {
  if (qrcode && status === types.ScanStatus.Waiting) {
    qrTerminal.generate(qrcode, { small: true })
  }
  console.info(`Login status: ${types.ScanStatus[status]}`)
})
puppet.on('login', async ({ contactId }) => {
  const self = await puppet.contactPayload(contactId)
  console.info(`Logged in as ${self.name}.`)
})
puppet.on('ready', () => console.info('Ready. Send "ding" to this account, or in a group with it, to receive "dong".'))
puppet.on('logout', ({ data }) => console.info(`Logged out: ${data}`))
puppet.on('error', ({ gerror }) => console.error(gerror ? JSON.parse(gerror).message : 'Unknown error'))
puppet.on('message', async ({ messageId }) => {
  try {
    const message = await puppet.messagePayload(messageId)
    console.info(`Received${message.roomId ? ' in a group' : ''}: ${message.text ?? ''}`)
    if (message.type === types.Message.Text && message.talkerId !== puppet.currentUserId && message.text === 'ding') {
      // Reply in the group the message came from, or directly to the person who sent it.
      await puppet.messageSendText(message.roomId ?? message.talkerId, 'dong')
      console.info(`Sent${message.roomId ? ' in the group' : ''}: dong`)
    }
  } catch (error) {
    console.error(error)
  }
})

process.once('SIGINT', () => { void puppet.stop().catch(console.error) })
process.once('SIGTERM', () => { void puppet.stop().catch(console.error) })
await puppet.start()
