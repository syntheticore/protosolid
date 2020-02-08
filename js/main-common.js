import Vue from 'vue';
import { library } from '@fortawesome/fontawesome-svg-core'
import { faCaretDown, faCaretRight, faAngleDown, faAtom, faAsterisk, faBackspace, faBell, faBookmark, faBox, faBoxes, faCheckCircle, faCloud, faCodeBranch, faCog, faExpand, faCompress, faEye, faLink, faMagnet, faThumbtack, faTools, faTrash, faUser, faUserCircle, faPlus, faRuler, faPlay, faPause, faStop, faPen, faMousePointer, faMouse, faLock, faLockOpen, faHashtag, faTimesCircle, faTimes } from '@fortawesome/free-solid-svg-icons'
import { FontAwesomeIcon } from '@fortawesome/vue-fontawesome'

library.add(faCaretDown, faCaretRight, faAngleDown, faAtom, faAsterisk, faBackspace, faBell, faBookmark, faBox, faBoxes, faCheckCircle, faCloud, faCodeBranch, faCog, faExpand, faCompress, faEye, faLink, faMagnet, faThumbtack, faTools, faTrash, faUser, faUserCircle, faPlus, faRuler, faPlay, faPause, faStop, faPen, faMousePointer, faMouse, faLock, faLockOpen, faHashtag, faTimesCircle, faTimes);
Vue.component('fa-icon', FontAwesomeIcon);
