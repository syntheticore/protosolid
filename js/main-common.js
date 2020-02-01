import Vue from 'vue';
import { library } from '@fortawesome/fontawesome-svg-core'
import { faCaretDown, faAngleDown, faAtom, faAsterisk, faBackspace, faBell, faBookmark, faBox, faBoxes, faCheckCircle, faCloud, faCodeBranch, faCog, faExpand, faCompress, faEye, faLink, faMagnet, faThumbtack, faTools, faTrash, faUser, faRuler, faPlay, faPause, faStop, faPen, faMousePointer, faMouse, faLock, faLockOpen, faHashtag } from '@fortawesome/free-solid-svg-icons'
import { FontAwesomeIcon } from '@fortawesome/vue-fontawesome'

library.add(faCaretDown, faAngleDown, faAtom, faAsterisk, faBackspace, faBell, faBookmark, faBox, faBoxes, faCheckCircle, faCloud, faCodeBranch, faCog, faExpand, faCompress, faEye, faLink, faMagnet, faThumbtack, faTools, faTrash, faUser, faRuler, faPlay, faPause, faStop, faPen, faMousePointer, faMouse, faLock, faLockOpen, faHashtag);
Vue.component('fa-icon', FontAwesomeIcon);
